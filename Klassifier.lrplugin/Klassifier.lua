--[[----------------------------------------------------------------------------
  Klassifier.lua
  Asks for a label set and optional hints (Options.lua), runs klassifier.exe on the
  targeted photos in chunks, and opens the review window (Review.lua) with the keywords
  it suggests for each one. Save Keywords writes one photo's, Save All the rest (Keywords.lua). Thumbnails for
  the review are made along the way (Thumbs.lua).
------------------------------------------------------------------------------]]

local LrApplication     = import 'LrApplication'
local LrDialogs         = import 'LrDialogs'
local LrFileUtils       = import 'LrFileUtils'
local LrFunctionContext = import 'LrFunctionContext'
local LrLogger          = import 'LrLogger'
local LrPathUtils       = import 'LrPathUtils'
local LrProgressScope   = import 'LrProgressScope'

local Exec     = require 'Exec'
local Keywords = require 'Keywords'
local Options  = require 'Options'
local Review   = require 'Review'
local Thumbs   = require 'Thumbs'

local logger = LrLogger('Klassifier')
logger:enable('logfile') -- writes to Klassifier.log in the Lightroom logs folder

-- Photos per klassifier.exe run. Cancel is checked between runs.
local CHUNK_SIZE = 100
-- Labels reported per photo: the candidates offered for review.
local TOP_LABELS = 10

-- Runs the app on one chunk of paths. `labelArgs` are the --labels options, `thumbArgs` the
-- --thumbs ones. Returns a table of path -> row fields plus what it ran on ("DirectML (<GPU
-- name>)" or "CPU"), or nil plus an error.
local function processChunk(paths, labelArgs, thumbArgs)
	local listFile = Exec.tempFile('klassifier-list.txt')
	assert(Exec.writeLines(listFile, paths))

	local output, err = Exec.run('klassifier.exe', string.format('%s %s --list "%s"', labelArgs, thumbArgs, listFile))
	LrFileUtils.delete(listFile)
	if not output then return nil, err end
	logger:info(output)

	local rows, backend = {}, nil
	for line in output:gmatch('[^\r\n]+') do
		local fields = Exec.splitTabs(line)
		-- Fields: status, path, ... (extra fields from later versions are ignored).
		-- Other lines are comments (#...) or warnings from the app.
		if fields[1] == 'OK' or fields[1] == 'ERR' then
			rows[fields[2]] = fields
		elseif fields[1] == '#klassifier' then
			local gpu
			for _, field in ipairs(fields) do
				backend = field:match('^backend=(.+)$') or backend
				gpu = field:match('^gpu=(.+)$') or gpu
			end
			if backend and gpu then backend = backend .. ' (' .. gpu .. ')' end
		end
	end
	return rows, backend
end

-- "dog;beach" -> { 'dog', 'beach' }
local function splitList(field)
	local items = {}
	for item in (field or ''):gmatch('[^;]+') do items[#items + 1] = item end
	return items
end

-- Counts one result's failure, if it failed, and adds its report line (see showReport in
-- Review.lua; the report only opens when no photo classified).
local function describe(path, fields, counts, lines)
	local name = LrPathUtils.leafName(path)
	if not fields then
		counts.failed = counts.failed + 1
		lines[#lines + 1] = name .. ': no result from klassifier'
	elseif fields[1] ~= 'OK' then
		local message = fields[3] or 'unknown error'
		if message:sub(1, 12) == 'unsupported:' then
			counts.unsupported = counts.unsupported + 1
		else
			counts.failed = counts.failed + 1
		end
		lines[#lines + 1] = name .. ': ' .. message
	end
end

-- Builds the --labels options for the chosen set and hints. Hints go in a temp file (deleted
-- when `context` ends) so they never pass through the command line.
local function labelArguments(context, options)
	local args = { '--top ' .. TOP_LABELS }
	if not options.onlyHints then
		local setFile = LrPathUtils.child(LrPathUtils.child(_PLUGIN.path, Options.LABEL_SET_DIR), options.labelSet .. '.txt')
		args[#args + 1] = string.format('--labels "%s"', setFile)
	end
	if #options.hints > 0 then
		local hintsFile = Exec.tempFile('klassifier-hints.txt')
		assert(Exec.writeLines(hintsFile, options.hints))
		context:addCleanupHandler(function() LrFileUtils.delete(hintsFile) end)
		args[#args + 1] = string.format('--labels "%s"', hintsFile)
	end
	return table.concat(args, ' ')
end

local function describeLabels(options)
	local hints = table.concat(options.hints, ', ')
	if options.onlyHints then return 'hints only (' .. hints .. ')' end
	if hints == '' then return options.labelSet .. ' label set' end
	return options.labelSet .. ' label set + hints (' .. hints .. ')'
end

-- The target photos to classify, one entry per file: { path, photos, virtualCopies }.
-- Virtual copies that share a file with another target photo go in the same entry, with
-- the original (if targeted) first. `virtualCopies` counts them. Also returns the video count.
local function collectTargets(catalog)
	-- getTargetPhotos() returns the selection, or the whole filmstrip if nothing is selected.
	local photos = catalog:getTargetPhotos()
	local metadata = catalog:batchGetRawMetadata(photos, { 'path', 'isVideo', 'isVirtualCopy' })
	local entries, byPath, videos = {}, {}, 0
	for _, photo in ipairs(photos) do
		local m = metadata[photo]
		if m.isVideo then
			videos = videos + 1
		else
			local entry = byPath[m.path]
			if not entry then
				entry = { path = m.path, photos = {}, virtualCopies = 0 }
				byPath[m.path] = entry
				entries[#entries + 1] = entry
			end
			if m.isVirtualCopy then
				entry.virtualCopies = entry.virtualCopies + 1
				table.insert(entry.photos, photo)
			else
				-- The original goes first: its thumbnail represents the file.
				table.insert(entry.photos, 1, photo)
			end
		end
	end
	return entries, videos
end

-- Runs klassifier.exe on `paths` in chunks, with a progress bar, saving each photo's stand-in
-- thumbnail into `thumbs` (see Thumbs.lua). Returns a list of { path, fields } in `paths`
-- order, the backend name and whether the user cancelled. Returns nil if a run fails (after
-- showing why).
local function classifyAll(context, paths, labelArgs, thumbs)
	local progress = LrProgressScope { title = 'Classifying photos', functionContext = context }
	local results, cancelled, backend = {}, false, nil
	for first = 1, #paths, CHUNK_SIZE do
		if progress:isCanceled() then
			cancelled = true
			break
		end
		local last = math.min(first + CHUNK_SIZE - 1, #paths)
		progress:setCaption(string.format('Photos %d-%d of %d', first, last, #paths))

		local chunk = {}
		for i = first, last do chunk[#chunk + 1] = paths[i] end
		local thumbArgs = string.format('--thumbs "%s" --thumb-start %d', thumbs.dir, first)
		local rows, chunkBackend = processChunk(chunk, labelArgs, thumbArgs)
		if not rows then
			local err = chunkBackend
			progress:done()
			logger:error(err)
			LrDialogs.message('Classification report', 'Failed to run klassifier.exe:\n' .. err, 'critical')
			return nil
		end
		backend = chunkBackend or backend
		for _, path in ipairs(chunk) do
			results[#results + 1] = { path = path, fields = rows[path] }
		end
		progress:setPortionComplete(last, #paths)
	end
	progress:done()
	return results, backend, cancelled
end

-- "egret=Wildlife>Bird>Egret;owl=Wildlife>Bird>Owl" -> { egret = { 'Wildlife', 'Bird', 'Egret' }, ... }
local function parseKeywordPaths(field)
	local paths = {}
	for _, entry in ipairs(splitList(field)) do
		local label, path = entry:match('^(.-)=(.+)$')
		if label then
			local parts = {}
			for part in path:gmatch('[^>]+') do parts[#parts + 1] = part end
			paths[label] = parts
		end
	end
	return paths
end

-- Review items (see Review.lua) for the photos that classified OK, in `results` order.
-- `results` are in `entries` order, so a result's position is its thumbnail number.
local function buildItems(entries, results, thumbs)
	local byPath = {}
	for _, entry in ipairs(entries) do byPath[entry.path] = entry end
	local items = {}
	for n, r in ipairs(results) do
		local fields = r.fields
		if fields and fields[1] == 'OK' then
			-- Field 9 is "label=score;...", best first; 12 the suggested labels; 13 the keyword paths.
			local candidates, suggested = {}, {}
			for label in (fields[9] or ''):gmatch('([^;=]+)=[%d.]+') do candidates[#candidates + 1] = label end
			for _, label in ipairs(splitList(fields[12])) do suggested[label] = true end
			items[#items + 1] = {
				path = r.path,
				name = LrPathUtils.leafName(r.path),
				photos = byPath[r.path].photos,
				virtualCopies = byPath[r.path].virtualCopies,
				candidates = candidates,
				suggested = suggested,
				paths = parseKeywordPaths(fields[13]),
				standin = thumbs.standin(n),
				rendered = thumbs.rendered(n),
			}
		end
	end
	return items
end

-- What to tell the user when Classify Photos is clicked again, by what the first run is doing.
local BUSY = {
	options = 'The Classify Photos options window is already open. It may be behind Lightroom\'s window.',
	classifying = 'Classify Photos is already running. You can cancel it from the progress bar.',
	review = 'Classify Photos is already running: finish its review first. The review window may be behind Lightroom\'s window.',
	report = 'Classify Photos is showing its report. Close it first.',
}

-- True when two `entries` lists from collectTargets cover the same files with the same number
-- of photos each. Order doesn't matter: Lightroom may return the same selection reordered.
local function sameTargets(a, b)
	if #a ~= #b then return false end
	local photoCount = {}
	for _, entry in ipairs(b) do
		photoCount[entry.path] = #entry.photos
	end
	for _, entry in ipairs(a) do
		if photoCount[entry.path] ~= #entry.photos then return false end
	end
	return true
end

LrFunctionContext.postAsyncTaskWithContext('Klassifier', function(context)
	LrDialogs.attachErrorDialogToFunctionContext(context)
	-- This file runs afresh for each menu click, but required modules are shared, so the flag
	-- lives in Exec. A second Classify Photos is turned away instead of opening a second review.
	if Exec.running then
		LrDialogs.message('Classify Photos', BUSY[Exec.running], 'info')
		return
	end
	Exec.running = 'options'
	context:addCleanupHandler(function() Exec.running = nil end)

	local entries, videos = collectTargets(LrApplication.activeCatalog())
	local paths, duplicates = {}, 0
	for _, entry in ipairs(entries) do
		paths[#paths + 1] = entry.path
		duplicates = duplicates + #entry.photos - 1
	end

	if #paths == 0 then
		LrDialogs.message('Classification report', 'No photos to process.', 'info')
		return
	end
	local options = Options.ask(context, #paths)
	if not options then return end
	-- The options window doesn't block Lightroom, so the selection may have changed meanwhile.
	local now = collectTargets(LrApplication.activeCatalog())
	if not sameTargets(entries, now) then
		local answer = LrDialogs.confirm('The selection changed while the options window was open.',
			string.format('Classify the %d photo(s) the options window showed? To classify the new selection, cancel and run Classify Photos again.', #paths),
			'Classify', 'Cancel')
		if answer ~= 'ok' then return end
	end
	local labelArgs = labelArguments(context, options)
	-- Lightroom's thumbnails are requested while classifying, so most are ready for the review.
	Exec.running = 'classifying'
	local thumbs = Thumbs.start(context, entries)

	local results, backend, cancelled = classifyAll(context, paths, labelArgs, thumbs)
	if not results then return end

	local counts = { failed = 0, unsupported = 0 }
	local lines = {}
	for _, r in ipairs(results) do
		describe(r.path, r.fields, counts, lines)
	end

	local items = buildItems(entries, results, thumbs)
	if #items == 0 then
		-- Nothing to review: say why instead.
		local summary = string.format(
			'No photo could be classified.\n' ..
			'%d failed, %d unsupported. Skipped %d video(s), %d duplicate(s). Ran on %s.\n' ..
			'Labels: %s.',
			counts.failed, counts.unsupported, videos, duplicates, backend or 'unknown', describeLabels(options))
		if cancelled then
			summary = summary .. string.format('\nCancelled after %d of %d photos.', #results, #paths)
		end
		Exec.running = 'report'
		Review.showReport(context, summary, lines)
		return
	end

	local notes = string.format('Labels: %s. %d failed, %d unsupported. Skipped %d video(s), %d duplicate(s).',
		describeLabels(options), counts.failed, counts.unsupported, videos, duplicates)
	if cancelled then
		notes = notes .. string.format('\nCancelled after %d of %d photos.', #results, #paths)
	end
	-- Reopens the review (choices kept) if the catalog is busy when writing.
	local catalog = LrApplication.activeCatalog()
	local function saveItem(item) return Keywords.apply(catalog, { item }, Review.keywordPaths) end
	Exec.running = 'review'
	while true do
		local result = Review.show(context, items, notes, saveItem)
		if result ~= 'ok' then return end
		-- Photos saved with Save Keywords already have their keywords.
		local unsaved = Review.unsaved(items)
		if Review.totals(unsaved) == 0 then
			if #unsaved == #items then LrDialogs.message('Review keywords', 'No keywords to add.', 'info') end
			return
		end
		local assigned, photos = Keywords.apply(catalog, unsaved, Review.keywordPaths)
		if assigned then
			LrDialogs.message('Review keywords', string.format('Added %d keyword(s) to %d photo(s).', assigned, photos), 'info')
			return
		end
		LrDialogs.message('Review keywords',
			'Lightroom\'s catalog was busy, so no keywords were added. Your choices are kept: try Save All again.',
			'warning')
	end
end)
