--[[----------------------------------------------------------------------------
  HelloWorld.lua
  Runs hello-app.exe on the targeted photos, in chunks, and shows the labels
  it matched for each one.
------------------------------------------------------------------------------]]

local LrApplication     = import 'LrApplication'
local LrDialogs         = import 'LrDialogs'
local LrFileUtils       = import 'LrFileUtils'
local LrFunctionContext = import 'LrFunctionContext'
local LrLogger          = import 'LrLogger'
local LrPathUtils       = import 'LrPathUtils'
local LrProgressScope   = import 'LrProgressScope'
local LrTasks           = import 'LrTasks'
local LrView            = import 'LrView'

local logger = LrLogger('HelloWorld')
logger:enable('logfile') -- writes to HelloWorld.log in the Lightroom logs folder

-- Photos per hello-app.exe run. Cancel is checked between runs.
local CHUNK_SIZE = 100
-- Ask before processing more photos than this (e.g. a whole filmstrip with nothing selected).
local CONFIRM_ABOVE = 500

local function tempFile(name)
	return LrFileUtils.chooseUniqueFileName(LrPathUtils.child(LrPathUtils.getStandardFilePath('temp'), name))
end

-- Runs hello-app.exe with `args` (already quoted). Returns its output (stdout and stderr),
-- or nil plus an error message.
local function runHelloApp(args)
	local exe = LrPathUtils.child(_PLUGIN.path, 'hello-app.exe')
	local outFile = tempFile('hello-app-output.txt')

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
		return nil, 'hello-app.exe exited with code ' .. tostring(exitCode) .. '\n' .. detail
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

-- Runs the app on one chunk of paths. Returns a table of path -> row fields plus the backend
-- name (DirectML or CPU), or nil plus an error.
local function processChunk(paths)
	local listFile = tempFile('hello-app-list.txt')
	local f = assert(io.open(listFile, 'wb'))
	f:write(table.concat(paths, '\n'), '\n')
	f:close()

	local output, err = runHelloApp(string.format('--list "%s"', listFile))
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
		elseif fields[1] == '#hello-app' then
			for _, field in ipairs(fields) do
				backend = field:match('^backend=(.+)$') or backend
			end
		end
	end
	return rows, backend
end

-- "dog=0.1234;beach=0.0567" -> "dog 12.3%, beach 5.7%"
local function formatLabels(field)
	local parts = {}
	for label, score in (field or ''):gmatch('([^;=]+)=([%d.]+)') do
		parts[#parts + 1] = string.format('%s %.1f%%', label, tonumber(score) * 100)
	end
	return #parts > 0 and table.concat(parts, ', ') or '(no labels)'
end

-- Turns one result into a report line and updates the counts.
local function describe(path, fields, counts)
	local name = LrPathUtils.leafName(path)
	if not fields then
		counts.failed = counts.failed + 1
		return name .. ': no result from hello-app'
	elseif fields[1] == 'OK' then
		-- OK, path, source, width, height, rotation, ms, flags, labels
		local source, width, height, ms, flags, labels = fields[3], fields[4], fields[5], fields[7], fields[8], fields[9]
		counts.loaded = counts.loaded + 1
		local line = string.format('%s:  %s    [%sx%s %s, %s ms]', name, formatLabels(labels), width, height, source, ms)
		if flags and flags:find('small', 1, true) then
			counts.small = counts.small + 1
			line = line .. ' [SMALL PREVIEW]'
		end
		return line
	else
		local message = fields[3] or 'unknown error'
		if message:sub(1, 12) == 'unsupported:' then
			counts.unsupported = counts.unsupported + 1
		else
			counts.failed = counts.failed + 1
		end
		return name .. ': ' .. message
	end
end

local function showReport(summary, lines)
	local f = LrView.osFactory()
	LrDialogs.presentModalDialog {
		title = 'Classification report',
		contents = f:column {
			spacing = f:control_spacing(),
			f:static_text { title = summary },
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

LrFunctionContext.postAsyncTaskWithContext('HelloWorld', function(context)
	LrDialogs.attachErrorDialogToFunctionContext(context)

	local catalog = LrApplication.activeCatalog()
	-- getTargetPhotos() returns the selection, or the whole filmstrip if nothing is selected.
	local photos = catalog:getTargetPhotos()
	local metadata = catalog:batchGetRawMetadata(photos, { 'path', 'isVideo' })

	-- Skip videos, and virtual copies that share a file with a photo already listed.
	local paths, seen, videos, duplicates = {}, {}, 0, 0
	for _, photo in ipairs(photos) do
		local m = metadata[photo]
		if m.isVideo then
			videos = videos + 1
		elseif seen[m.path] then
			duplicates = duplicates + 1
		else
			seen[m.path] = true
			paths[#paths + 1] = m.path
		end
	end

	if #paths == 0 then
		LrDialogs.message('Classification report', 'No photos to process.', 'info')
		return
	end
	if #paths > CONFIRM_ABOVE then
		local answer = LrDialogs.confirm(
			string.format('Classify %d photos?', #paths),
			'This can take a while. You can cancel from the progress bar.')
		if answer ~= 'ok' then return end
	end

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
		local rows, chunkBackend = processChunk(chunk)
		if not rows then
			local err = chunkBackend
			progress:done()
			logger:error(err)
			LrDialogs.message('Classification report', 'Failed to run hello-app.exe:\n' .. err, 'critical')
			return
		end
		backend = chunkBackend or backend
		for _, path in ipairs(chunk) do
			results[#results + 1] = { path = path, fields = rows[path] }
		end
		progress:setPortionComplete(last, #paths)
	end
	progress:done()

	local counts = { loaded = 0, small = 0, failed = 0, unsupported = 0 }
	local lines = {}
	for _, r in ipairs(results) do
		lines[#lines + 1] = describe(r.path, r.fields, counts)
	end

	local summary = string.format(
		'%d classified (%d with small previews), %d failed, %d unsupported. Skipped %d video(s), %d duplicate(s).\n' ..
		'Ran on %s. Each label is scored on its own; compare scores within a photo.',
		counts.loaded, counts.small, counts.failed, counts.unsupported, videos, duplicates, backend or 'unknown')
	if cancelled then
		summary = summary .. string.format('\nCancelled after %d of %d photos.', #results, #paths)
	end
	showReport(summary, lines)
end)
