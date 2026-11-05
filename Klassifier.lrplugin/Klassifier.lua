--[[----------------------------------------------------------------------------
  Klassifier.lua
  Asks for a model, a label set and optional hints, runs klassifier.exe on the
  targeted photos in chunks, and opens the review window (Review.lua) with the
  keywords it suggests for each one. Accept All writes them (Keywords.lua).
------------------------------------------------------------------------------]]

local LrApplication     = import 'LrApplication'
local LrBinding         = import 'LrBinding'
local LrDialogs         = import 'LrDialogs'
local LrFileUtils       = import 'LrFileUtils'
local LrFunctionContext = import 'LrFunctionContext'
local LrLogger          = import 'LrLogger'
local LrPathUtils       = import 'LrPathUtils'
local LrPrefs           = import 'LrPrefs'
local LrProgressScope   = import 'LrProgressScope'
local LrTasks           = import 'LrTasks'
local LrView            = import 'LrView'

local Keywords = require 'Keywords'
local Review   = require 'Review'

local logger = LrLogger('Klassifier')
logger:enable('logfile') -- writes to Klassifier.log in the Lightroom logs folder

-- Photos per klassifier.exe run. Cancel is checked between runs.
local CHUNK_SIZE = 100
-- Labels reported per photo: the candidates offered for review.
local TOP_LABELS = 10
-- Warn in the options dialog above this many photos (e.g. a whole filmstrip with nothing selected).
local WARN_ABOVE = 500
-- Label sets are the .txt files in this folder inside the plugin.
local LABEL_SET_DIR = 'label-sets'
local DEFAULT_LABEL_SET = 'General'
-- The models offered in the options dialog, the default first. `name` is klassifier.exe's
-- --model value and the model's folder in models/ (see MODELS in klassifier/src/siglip.rs).
local MODELS = {
	{ name = 'siglip-base-224', title = 'Light (SigLIP base, fastest)' },
	{ name = 'siglip2-so400m-fp16', title = 'Medium (SigLIP 2 So400m, fp16)' },
	{ name = 'siglip2-so400m-fp32', title = 'High (SigLIP 2 So400m, fp32)' },
}

local function tempFile(name)
	return LrFileUtils.chooseUniqueFileName(LrPathUtils.child(LrPathUtils.getStandardFilePath('temp'), name))
end

-- Runs klassifier.exe with `args` (already quoted). Returns its output (stdout and stderr),
-- or nil plus an error message.
local function runKlassifier(args)
	local exe = LrPathUtils.child(_PLUGIN.path, 'klassifier.exe')
	local outFile = tempFile('klassifier-output.txt')

	local cmd = string.format('"%s" %s > "%s" 2>&1', exe, args, outFile)
	-- cmd /c needs the whole command wrapped in quotes when it contains quoted paths.
	if WIN_ENV then cmd = '"' .. cmd .. '"' end

	logger:info('Running: ' .. cmd)
	local exitCode = LrTasks.execute(cmd)
	local output = LrFileUtils.readFile(outFile)
	LrFileUtils.delete(outFile)
	if exitCode ~= 0 then
		-- The app prints "error: ..." explaining why; show the end of its output.
		local detail = output and output:gsub('%s+$', ''):sub(-1000) or ''
		return nil, 'klassifier.exe exited with code ' .. tostring(exitCode) .. '\n' .. detail
	end
	if not output then
		return nil, 'Could not read output file ' .. outFile
	end
	return output
end

local function splitTabs(line)
	local fields = {}
	for field in (line .. '\t'):gmatch('([^\t]*)\t') do
		fields[#fields + 1] = field
	end
	return fields
end

-- Runs the app on one chunk of paths. `appArgs` are its other options (appArguments).
-- Returns a table of path -> row fields plus what it ran on ("DirectML (<GPU name>)" or
-- "CPU"), or nil plus an error.
local function processChunk(paths, appArgs)
	local listFile = tempFile('klassifier-list.txt')
	local f = assert(io.open(listFile, 'wb'))
	f:write(table.concat(paths, '\n'), '\n')
	f:close()

	local output, err = runKlassifier(string.format('%s --list "%s"', appArgs, listFile))
	LrFileUtils.delete(listFile)
	if not output then return nil, err end
	logger:info(output)

	local rows, backend = {}, nil
	for line in output:gmatch('[^\r\n]+') do
		local fields = splitTabs(line)
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

-- Label shares "dog=0.9213;cat=0.0712" -> "dog 92%, cat 7%"
local function formatShares(field)
	local parts = {}
	for label, share in (field or ''):gmatch('([^;=]+)=([%d.]+)') do
		local percent = tonumber(share) * 100
		parts[#parts + 1] = percent < 0.5 and (label .. ' <1%') or string.format('%s %.0f%%', label, percent)
	end
	return #parts > 0 and table.concat(parts, ', ') or '(no labels)'
end

-- "dog;beach" -> { 'dog', 'beach' }
local function splitList(field)
	local items = {}
	for item in (field or ''):gmatch('[^;]+') do items[#items + 1] = item end
	return items
end

-- Suggested keywords "dog;beach" -> "dog, beach"
local function formatSuggested(field)
	local labels = splitList(field)
	return #labels > 0 and table.concat(labels, ', ') or 'none'
end

-- Adds the report lines for one result and updates the counts.
local function describe(path, fields, counts, lines)
	local name = LrPathUtils.leafName(path)
	if not fields then
		counts.failed = counts.failed + 1
		lines[#lines + 1] = name .. ': no result from klassifier'
	elseif fields[1] == 'OK' then
		-- OK, path, source, width, height, rotation, ms, flags, labels, shares, match, suggested
		local source, width, height, ms, flags = fields[3], fields[4], fields[5], fields[7], fields[8]
		local shares, match, suggested = fields[10], fields[11] or 'unknown', fields[12]
		counts.loaded = counts.loaded + 1
		counts[match] = (counts[match] or 0) + 1
		local heading = string.format('%s  -  %s match    (%sx%s %s, %s ms)', name, match, width, height, source, ms)
		if flags and flags:find('small', 1, true) then
			counts.small = counts.small + 1
			heading = heading .. '  SMALL PREVIEW'
		end
		lines[#lines + 1] = heading
		lines[#lines + 1] = '        ' .. formatShares(shares)
		lines[#lines + 1] = '        Suggested: ' .. formatSuggested(suggested)
	else
		local message = fields[3] or 'unknown error'
		if message:sub(1, 12) == 'unsupported:' then
			counts.unsupported = counts.unsupported + 1
		else
			counts.failed = counts.failed + 1
		end
		lines[#lines + 1] = name .. ': ' .. message
	end
end

-- Names of the label sets (the .txt files in LABEL_SET_DIR), the default first.
local function listLabelSets()
	local names = {}
	for file in LrFileUtils.files(LrPathUtils.child(_PLUGIN.path, LABEL_SET_DIR)) do
		local leaf = LrPathUtils.leafName(file)
		if leaf:lower():match('%.txt$') then
			names[#names + 1] = LrPathUtils.removeExtension(leaf)
		end
	end
	table.sort(names, function(a, b)
		if a == DEFAULT_LABEL_SET or b == DEFAULT_LABEL_SET then return a == DEFAULT_LABEL_SET end
		return a:lower() < b:lower()
	end)
	return names
end

-- The MODELS whose folder is in the plugin (all of them, if none is, so the app explains).
local function listModels()
	local installed = {}
	for _, model in ipairs(MODELS) do
		if LrFileUtils.exists(LrPathUtils.child(LrPathUtils.child(_PLUGIN.path, 'models'), model.name)) then
			installed[#installed + 1] = model
		end
	end
	return #installed > 0 and installed or MODELS
end

local function modelTitle(name)
	for _, model in ipairs(MODELS) do
		if model.name == name then return model.title end
	end
	return name
end

-- "makeup, hair styling,,backstage" -> { 'makeup', 'hair styling', 'backstage' }
local function parseHints(text)
	local hints = {}
	for hint in (text or ''):gmatch('[^,;\r\n]+') do
		hint = hint:match('^%s*(.-)%s*$')
		if hint ~= '' then hints[#hints + 1] = hint end
	end
	return hints
end

-- Shows the options dialog. Returns { model, labelSet, hints, onlyHints }, or nil if cancelled.
-- The choices are remembered for next time.
local function askOptions(context, photoCount)
	local prefs = LrPrefs.prefsForPlugin()
	local sets = listLabelSets()
	if #sets == 0 then
		LrDialogs.message('Classify Photos', 'No label sets found in the plugin\'s ' .. LABEL_SET_DIR .. ' folder.', 'critical')
		return nil
	end

	local models = listModels()
	local props = LrBinding.makePropertyTable(context)
	props.model = models[1].name
	for _, model in ipairs(models) do
		if model.name == prefs.model then props.model = model.name end
	end
	props.labelSet = sets[1]
	for _, name in ipairs(sets) do
		if name == prefs.labelSet then props.labelSet = name end
	end
	props.hints = prefs.hints or ''
	props.onlyHints = prefs.onlyHints or false

	local f = LrView.osFactory()
	local bind = LrView.bind
	local labelWidth = LrView.share 'label_width'
	local items = {}
	for _, name in ipairs(sets) do items[#items + 1] = { title = name, value = name } end
	local modelItems = {}
	for _, model in ipairs(models) do modelItems[#modelItems + 1] = { title = model.title, value = model.name } end

	local rows = {
		bind_to_object = props,
		spacing = f:control_spacing(),
		f:row {
			f:static_text { title = 'Model:', alignment = 'right', width = labelWidth },
			f:popup_menu { items = modelItems, value = bind 'model' },
		},
		f:row {
			f:static_text { title = 'Label set:', alignment = 'right', width = labelWidth },
			f:popup_menu { items = items, value = bind 'labelSet', enabled = LrBinding.negativeOfKey 'onlyHints' },
		},
		f:row {
			f:static_text { title = 'Hints:', alignment = 'right', width = labelWidth },
			f:edit_field { value = bind 'hints', width_in_chars = 45, immediate = true },
		},
		f:row {
			f:spacer { width = labelWidth },
			f:static_text { title = 'Extra labels for this run, separated by commas,\ne.g. makeup, hair styling, backstage', height_in_lines = 2 },
		},
		f:row {
			f:spacer { width = labelWidth },
			f:checkbox { title = 'Use only the hints, not the label set', value = bind 'onlyHints' },
		},
	}
	if photoCount > WARN_ABOVE then
		rows[#rows + 1] = f:static_text {
			title = string.format('%d photos can take a while. You can cancel from the progress bar.', photoCount),
		}
	end

	local result = LrDialogs.presentModalDialog {
		title = 'Classify Photos',
		contents = f:column(rows),
		actionVerb = string.format('Classify %d photo%s', photoCount, photoCount == 1 and '' or 's'),
	}
	if result ~= 'ok' then return nil end

	-- Saved before checking, so a rejected entry is still there to fix next time.
	prefs.model, prefs.labelSet, prefs.hints, prefs.onlyHints = props.model, props.labelSet, props.hints, props.onlyHints
	local options = { model = props.model, labelSet = props.labelSet, hints = parseHints(props.hints), onlyHints = props.onlyHints }
	if options.onlyHints and #options.hints == 0 then
		LrDialogs.message('Classify Photos', 'Enter at least one hint, or untick "Use only the hints".', 'warning')
		return nil
	end
	-- The hints file is read like a label set, so "=>" would map a hint to a nested keyword.
	for _, hint in ipairs(options.hints) do
		if hint:find('=>', 1, true) then
			LrDialogs.message('Classify Photos',
				'Hints can\'t map to nested keywords: remove "=>" from "' .. hint .. '".\n' ..
				'Put mappings in a label set file instead.', 'warning')
			return nil
		end
	end
	return options
end

-- Builds klassifier.exe's options for the chosen model, set and hints. Hints go in a temp file
-- (deleted when `context` ends) so they never pass through the command line.
local function appArguments(context, options)
	local args = { '--top ' .. TOP_LABELS, '--model ' .. options.model }
	if not options.onlyHints then
		local setFile = LrPathUtils.child(LrPathUtils.child(_PLUGIN.path, LABEL_SET_DIR), options.labelSet .. '.txt')
		args[#args + 1] = string.format('--labels "%s"', setFile)
	end
	if #options.hints > 0 then
		local hintsFile = tempFile('klassifier-hints.txt')
		local f = assert(io.open(hintsFile, 'wb'))
		f:write(table.concat(options.hints, '\n'), '\n')
		f:close()
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

local function showReport(summary, lines)
	local f = LrView.osFactory()
	LrDialogs.presentModalDialog {
		title = 'Classification report',
		contents = f:column {
			spacing = f:control_spacing(),
			f:static_text { title = summary, height_in_lines = select(2, summary:gsub('\n', '')) + 1 },
			f:scrolled_view {
				width = 720,
				height = 420,
				f:static_text {
					title = table.concat(lines, '\n'),
					height_in_lines = #lines,
					width_in_chars = 110,
				},
			},
		},
		actionVerb = 'OK',
		cancelVerb = '< exclude >',
	}
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

-- Runs klassifier.exe on `paths` in chunks, with a progress bar. Returns a list of
-- { path, fields } in `paths` order, the backend name and whether the user cancelled.
-- Returns nil if a run fails (after showing why).
local function classifyAll(context, paths, appArgs)
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
		local rows, chunkBackend = processChunk(chunk, appArgs)
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
local function buildItems(entries, results)
	local byPath = {}
	for _, entry in ipairs(entries) do byPath[entry.path] = entry end
	local items = {}
	for _, r in ipairs(results) do
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
			}
		end
	end
	return items
end

LrFunctionContext.postAsyncTaskWithContext('Klassifier', function(context)
	LrDialogs.attachErrorDialogToFunctionContext(context)

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
	local options = askOptions(context, #paths)
	if not options then return end
	local appArgs = appArguments(context, options)

	local results, backend, cancelled = classifyAll(context, paths, appArgs)
	if not results then return end

	local counts = { loaded = 0, small = 0, failed = 0, unsupported = 0, strong = 0, good = 0, weak = 0 }
	local lines = {}
	for _, r in ipairs(results) do
		describe(r.path, r.fields, counts, lines)
	end

	local summary = string.format(
		'%d classified: %d strong, %d good, %d weak matches (%d with small previews).\n' ..
		'%d failed, %d unsupported. Skipped %d video(s), %d duplicate(s). Ran on %s.\n' ..
		'Model: %s. Labels: %s.\n\n' ..
		'Percentages are each label\'s share of the photo\'s total score. The match level\n' ..
		'says how sure the model is about the top label.',
		counts.loaded, counts.strong, counts.good, counts.weak, counts.small,
		counts.failed, counts.unsupported, videos, duplicates, backend or 'unknown',
		modelTitle(options.model), describeLabels(options))
	if cancelled then
		summary = summary .. string.format('\nCancelled after %d of %d photos.', #results, #paths)
	end

	local items = buildItems(entries, results)
	if #items == 0 then
		showReport(summary, lines)
		return
	end

	local notes = string.format('Model: %s. Labels: %s.\n%d failed, %d unsupported. Skipped %d video(s), %d duplicate(s).',
		modelTitle(options.model), describeLabels(options), counts.failed, counts.unsupported, videos, duplicates)
	if cancelled then
		notes = notes .. string.format('\nCancelled after %d of %d photos.', #results, #paths)
	end
	-- Reopens the review (choices kept) if the catalog is busy when writing.
	local catalog = LrApplication.activeCatalog()
	while true do
		local result = Review.show(context, items, notes, function() showReport(summary, lines) end)
		if result ~= 'ok' then return end
		if Review.totals(items) == 0 then
			LrDialogs.message('Review keywords', 'No keywords to add.', 'info')
			return
		end
		local assigned, photos = Keywords.apply(catalog, items, Review.keywordPaths)
		if assigned then
			LrDialogs.message('Review keywords', string.format('Added %d keyword(s) to %d photo(s).', assigned, photos), 'info')
			return
		end
		LrDialogs.message('Review keywords',
			'Lightroom\'s catalog was busy, so no keywords were added. Your choices are kept: try Accept All again.',
			'warning')
	end
end)
