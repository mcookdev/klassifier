--[[----------------------------------------------------------------------------
  Review.lua
  The review window: klassifier-ui shows the classified photos in a scrolling grid
  with their suggested keywords, a side panel to change and save a photo's keywords,
  and Save All / Cancel. This module writes its input, runs it and reads back the
  result, including the changes (format in klassifier/src/ui_model.rs). While the
  window is open it also saves photos as the window asks (Save Keywords). It also
  shows the report for a run where nothing could be classified.

  An item is one classified file:
    { path, name, photos = { LrPhoto... },   -- photos sharing the file, original first
      virtualCopies = n,                     -- how many of `photos` are virtual copies
      candidates = { label... },             -- the reported labels, best first
      suggested = { [label] = true },        -- the classifier's suggestions
      paths = { [label] = { name... } },     -- mapped keywords (label => Parent > Child)
      standin, rendered,                     -- thumbnail files (see Thumbs.lua)
      chosen = nil | { [label] = true },     -- the user's choice, once edited
      saved = nil | true }                   -- its keywords were written by Save Keywords
------------------------------------------------------------------------------]]

local LrDialogs   = import 'LrDialogs'
local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'
local LrTasks     = import 'LrTasks'

local Exec = require 'Exec'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Review = {}

-- The labels to write for `item`: its chosen ones if edited, else the suggested ones,
-- in candidate order.
function Review.keywordsFor(item)
	local set = item.chosen or item.suggested
	local labels = {}
	for _, label in ipairs(item.candidates) do
		if set[label] then labels[#labels + 1] = label end
	end
	return labels
end

-- The keyword for `label` as a list of names from the top level down: its mapping from the
-- labels file, or else a top-level keyword named as the label.
local function pathFor(item, label)
	return (item.paths and item.paths[label]) or { label }
end

-- The keywords to write for `item`, each as a list of names from the top level down.
function Review.keywordPaths(item)
	local paths = {}
	for _, label in ipairs(Review.keywordsFor(item)) do paths[#paths + 1] = pathFor(item, label) end
	return paths
end

-- The items Save All writes: all but those saved with Save Keywords.
function Review.unsaved(items)
	local unsaved = {}
	for _, item in ipairs(items) do
		if not item.saved then unsaved[#unsaved + 1] = item end
	end
	return unsaved
end

-- Keyword assignments and photos that would get at least one, over all items.
function Review.totals(items)
	local keywords, photos = 0, 0
	for _, item in ipairs(items) do
		local n = #Review.keywordsFor(item)
		if n > 0 then
			keywords = keywords + n * #item.photos
			photos = photos + #item.photos
		end
	end
	return keywords, photos
end

-- "+1 virtual copy" when the original and its copies are targeted, "virtual copy" when only
-- copies are, "" for an ordinary photo.
function Review.copyNote(item)
	local copies = item.virtualCopies or 0
	if copies == 0 then return '' end
	local noun = copies == 1 and 'virtual copy' or (copies .. ' virtual copies')
	if copies == #item.photos then return noun end
	return '+' .. (copies == 1 and '1 virtual copy' or noun)
end

-- The review window's input lines for `items` (ui_model.rs describes them). `notes` is as for
-- Review.show. `saveFiles` are the requests and answers files for Save Keywords, `aliveFile`
-- the file keepAlive rewrites.
local function inputLines(items, notes, saveFiles, aliveFile)
	local lines = { '#klassifier-ui\treview\t1' }
	Exec.addSavedWindow(lines, 'reviewWindow')
	if notes.report then
		lines[#lines + 1] = table.concat({
			'NOTES', Exec.escape(notes.before), Exec.escape(notes.failed), Exec.escape(notes.after) }, '\t')
		lines[#lines + 1] = 'REPORT\t' .. Exec.escape(notes.report.summary)
		for _, line in ipairs(notes.report.lines) do lines[#lines + 1] = 'LINE\t' .. Exec.clean(line) end
	else
		lines[#lines + 1] = 'NOTES\t' .. Exec.escape(notes.before .. notes.failed .. notes.after)
	end
	lines[#lines + 1] = 'SAVEFILES\t' .. Exec.clean(saveFiles.requests) .. '\t' .. Exec.clean(saveFiles.answers)
	lines[#lines + 1] = 'ALIVE\t' .. Exec.clean(aliveFile)
	for n, item in ipairs(items) do
		local set = item.chosen or item.suggested
		lines[#lines + 1] = table.concat({
			'ITEM', n, Exec.clean(item.name), Exec.clean(Review.copyNote(item)), #item.photos,
			item.chosen and 1 or 0, Exec.clean(item.standin), Exec.clean(item.rendered), item.saved and 1 or 0,
		}, '\t')
		for _, label in ipairs(item.candidates) do
			local path = item.paths and item.paths[label]
			lines[#lines + 1] = table.concat({
				'LABEL', n, Exec.clean(label), item.suggested[label] and 1 or 0, set[label] and 1 or 0,
				path and Exec.clean(table.concat(path, ' > ')) or '',
			}, '\t')
		end
	end
	return lines
end

-- Reads the window's output: sets `chosen` on the items it lists as edited, and returns
-- 'accept' or 'cancel', or nil if there is no RESULT line.
local function readResult(output, items)
	local result, edited = nil, {}
	for line in output:gmatch('[^\r\n]+') do
		local fields = Exec.splitTabs(line)
		local n = tonumber(fields[2])
		if fields[1] == 'RESULT' then
			result = fields[2]
		elseif fields[1] == 'EDITED' and items[n] then
			edited[n] = {}
		elseif fields[1] == 'CHOSEN' and edited[n] then
			-- The label's position in the item's LABEL lines, which follow its candidates.
			local label = items[n].candidates[tonumber(fields[3])]
			if label then edited[n][label] = true end
		end
	end
	for n, chosen in pairs(edited) do items[n].chosen = chosen end
	return result
end

-- True when the label sets `a` and `b` ({ [label] = true }) are the same.
local function sameSet(a, b)
	for label in pairs(a) do
		if not b[label] then return false end
	end
	for label in pairs(b) do
		if not a[label] then return false end
	end
	return true
end

-- Handles one Save Keywords request line (fields split): sets the item's choice, writes it with
-- `saveItem` and returns the answer line.
local function handleSave(fields, items, saveItem)
	local n = tonumber(fields[2])
	local item = n and items[n]
	if not item then return nil end
	if item.saved then return 'SAVED\t' .. n end
	local chosen = {}
	for i = 3, #fields do
		local label = item.candidates[tonumber(fields[i])]
		if label then chosen[label] = true end
	end
	-- Kept as the choice only if it differs from the suggestions, so a review reopened after
	-- Save All finds the catalog busy shows "(edited)" only on photos that were.
	if item.chosen or not sameSet(chosen, item.suggested) then item.chosen = chosen end
	local ok, assigned = LrTasks.pcall(saveItem, item)
	if not ok then
		logger:error('Save Keywords failed for ' .. item.name .. ': ' .. tostring(assigned))
		return 'FAILED\t' .. n .. '\t' .. Exec.clean(assigned)
	end
	if not assigned then
		return 'FAILED\t' .. n .. '\tLightroom\'s catalog was busy. Try again.'
	end
	item.saved = true
	logger:info(string.format('Save Keywords: %d keyword(s) on %s', assigned, item.name))
	return 'SAVED\t' .. n
end

-- Watches `files.requests` for Save Keywords requests while the window is open, saving each with
-- `saveItem(item)` (which returns the keyword count, or nil if the catalog was busy) and
-- appending the answer to `files.answers`. Returns a function that stops watching, once the
-- window has closed, after handling any request left.
local function watchSaves(files, items, saveItem)
	local stopped, done, handled = false, false, 0
	local function answer(line)
		local f = io.open(files.answers, 'ab')
		if not f then
			logger:error('Could not write ' .. files.answers)
			return
		end
		f:write(line, '\n')
		f:close()
	end
	local function poll()
		if not LrFileUtils.exists(files.requests) then return end
		local text = LrFileUtils.readFile(files.requests) or ''
		local count = 0
		-- Only complete lines: the window may be writing the last one.
		for line in text:gmatch('([^\n]*)\n') do
			count = count + 1
			if count > handled then
				handled = count
				local fields = Exec.splitTabs((line:gsub('\r$', '')))
				if fields[1] == 'SAVE' then
					local reply = handleSave(fields, items, saveItem)
					if reply then answer(reply) end
				end
			end
		end
	end
	LrTasks.startAsyncTask(function()
		while not stopped do
			local ok, err = LrTasks.pcall(poll)
			if not ok then logger:error('Save Keywords: ' .. tostring(err)) end
			LrTasks.sleep(0.1)
		end
		local ok, err = LrTasks.pcall(poll)
		if not ok then logger:error('Save Keywords: ' .. tostring(err)) end
		done = true
	end)
	return function()
		stopped = true
		while not done do LrTasks.sleep(0.05) end
	end
end

-- Rewrites `file` every second until stopped, so the review window can tell that Lightroom is
-- still there: it stops saving once the file goes stale. Writes it right away, so it exists
-- before the window starts. Its own task, so a slow save can't hold it up. Returns a function
-- that stops it.
local function keepAlive(file)
	local stopped, done, warned = false, false, false
	local function touch()
		local f = io.open(file, 'wb')
		if f then
			f:write('alive')
			f:close()
		elseif not warned then
			warned = true
			logger:error('Could not write ' .. file)
		end
	end
	touch()
	LrTasks.startAsyncTask(function()
		while not stopped do
			LrTasks.sleep(1)
			if not stopped then touch() end
		end
		done = true
	end)
	return function()
		stopped = true
		while not done do LrTasks.sleep(0.05) end
	end
end

-- Shows the review window for `items` (at least one). `notes` is the extra header text (label
-- set, skipped files, cancel): `before .. failed .. after`, where `failed` ("2 failed") links to
-- `report` ({ summary, lines }, as for showReport) if there is one. `saveItem(item)` writes one
-- item's keywords for Save Keywords: it returns the keyword count, or nil if the catalog was busy. Saved items get `saved = true`. Returns 'ok'
-- (Save All) or 'cancel'.
function Review.show(context, items, notes, saveItem)
	local inFile = Exec.tempFile('klassifier-review.txt')
	local saveFiles = { requests = Exec.tempFile('klassifier-save.txt'), answers = Exec.tempFile('klassifier-saved.txt') }
	local aliveFile = Exec.tempFile('klassifier-alive.txt')
	context:addCleanupHandler(function()
		LrFileUtils.delete(inFile)
		LrFileUtils.delete(saveFiles.requests)
		LrFileUtils.delete(saveFiles.answers)
		LrFileUtils.delete(aliveFile)
	end)
	local ok, err = Exec.writeLines(inFile, inputLines(items, notes, saveFiles, aliveFile))
	if not ok then
		LrDialogs.message('Review keywords', 'Could not write ' .. inFile .. ': ' .. tostring(err), 'critical')
		return 'cancel'
	end
	local stopAlive = keepAlive(aliveFile)
	local stopSaving = watchSaves(saveFiles, items, saveItem)
	local output, runErr = Exec.run('klassifier-ui', '--review --in ' .. Exec.quote(inFile))
	stopSaving()
	stopAlive()
	if output then Exec.saveWindow(output, 'reviewWindow') end
	local result = output and readResult(output, items)
	if not result then
		local why = runErr or ('No result from klassifier-ui:\n' .. (output or ''):sub(-1000))
		logger:error(why)
		LrDialogs.message('Review keywords', 'The review window failed:\n' .. why, 'critical')
		return 'cancel'
	end
	return result == 'accept' and 'ok' or 'cancel'
end

-- Shows the report for a run where no photo could be classified: `summary`, then `lines`
-- (each photo's error). Returns when it is closed.
function Review.showReport(context, summary, lines)
	local inFile = Exec.tempFile('klassifier-report.txt')
	context:addCleanupHandler(function() LrFileUtils.delete(inFile) end)
	local input = { '#klassifier-ui\treport\t1' }
	Exec.addSavedWindow(input, 'reportWindow')
	input[#input + 1] = 'SUMMARY\t' .. Exec.escape(summary)
	for _, line in ipairs(lines) do input[#input + 1] = 'LINE\t' .. Exec.clean(line) end
	local ok, err = Exec.writeLines(inFile, input)
	local output, runErr
	if ok then
		output, runErr = Exec.run('klassifier-ui', '--report --in ' .. Exec.quote(inFile))
	end
	if output then Exec.saveWindow(output, 'reportWindow') end
	if not output then
		-- The window failed: show the gist in a plain message instead.
		local why = runErr or ('Could not write ' .. inFile .. ': ' .. tostring(err))
		logger:error(why)
		LrDialogs.message('Classification report', summary .. '\n\n(The report window failed: ' .. why .. ')', 'warning')
	end
end

return Review
