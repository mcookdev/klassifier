--[[----------------------------------------------------------------------------
  Review.lua
  The review window: klassifier-ui.exe shows the classified photos in a scrolling grid
  with their suggested keywords, a side panel to change a photo's keywords, and
  Accept All / Cancel. This module writes its input, runs it and reads back the
  result, including the changes (format in klassifier/src/ui_model.rs).

  An item is one classified file:
    { path, name, photos = { LrPhoto... },   -- photos sharing the file, original first
      virtualCopies = n,                     -- how many of `photos` are virtual copies
      candidates = { label... },             -- the reported labels, best first
      suggested = { [label] = true },        -- the classifier's suggestions
      paths = { [label] = { name... } },     -- mapped keywords (label => Parent > Child)
      standin, rendered,                     -- thumbnail files (see Thumbs.lua)
      chosen = nil | { [label] = true } }    -- the user's choice, once edited
------------------------------------------------------------------------------]]

local LrDialogs   = import 'LrDialogs'
local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'

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

-- The review window's input lines for `items` (ui_model.rs describes them).
local function inputLines(items, notes)
	local lines = { '#klassifier-ui\treview\t1', 'NOTES\t' .. Exec.escape(notes) }
	for n, item in ipairs(items) do
		local set = item.chosen or item.suggested
		lines[#lines + 1] = table.concat({
			'ITEM', n, Exec.clean(item.name), Exec.clean(Review.copyNote(item)), #item.photos,
			item.chosen and 1 or 0, Exec.clean(item.standin), Exec.clean(item.rendered),
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

-- Shows the review window for `items` (at least one). `notes` is extra header text (skipped
-- files, cancel). Returns 'ok' (Accept All) or 'cancel'.
function Review.show(context, items, notes)
	local inFile = Exec.tempFile('klassifier-review.txt')
	context:addCleanupHandler(function() LrFileUtils.delete(inFile) end)
	local ok, err = Exec.writeLines(inFile, inputLines(items, notes))
	if not ok then
		LrDialogs.message('Review keywords', 'Could not write ' .. inFile .. ': ' .. tostring(err), 'critical')
		return 'cancel'
	end
	local output, runErr = Exec.run('klassifier-ui.exe', string.format('--review --in "%s"', inFile))
	local result = output and readResult(output, items)
	if not result then
		local why = runErr or ('No result from klassifier-ui.exe:\n' .. (output or ''):sub(-1000))
		logger:error(why)
		LrDialogs.message('Review keywords', 'The review window failed:\n' .. why, 'critical')
		return 'cancel'
	end
	return result == 'accept' and 'ok' or 'cancel'
end

return Review
