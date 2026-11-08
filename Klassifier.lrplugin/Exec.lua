--[[----------------------------------------------------------------------------
  Exec.lua
  Runs the plugin's apps (klassifier, klassifier-ui) and helps read and write the
  tab-separated lines they exchange with the plugin.
------------------------------------------------------------------------------]]

local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'
local LrPathUtils = import 'LrPathUtils'
local LrPrefs     = import 'LrPrefs'
local LrTasks     = import 'LrTasks'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Exec = {}

-- What Classify Photos is doing (see Klassifier.lua): nil when idle, else 'options', 'classifying', 'review' or 'report'.
Exec.running = nil

-- A new file name in the temp folder, based on `name`.
function Exec.tempFile(name)
	return LrFileUtils.chooseUniqueFileName(LrPathUtils.child(LrPathUtils.getStandardFilePath('temp'), name))
end

-- Each app's file in the plugin folder, by platform. On macOS klassifier-ui is in an app
-- bundle, so it has its name and icon in the Dock; running its binary directly still uses them.
local APPS = WIN_ENV and {
	['klassifier'] = 'klassifier.exe',
	['klassifier-ui'] = 'klassifier-ui.exe',
} or {
	['klassifier'] = 'klassifier',
	['klassifier-ui'] = 'Klassifier.app/Contents/MacOS/klassifier-ui',
}

-- `path` quoted as one shell argument. Windows paths can't contain '"'. On macOS the shell
-- expands $, ` and \ inside double quotes, so single quotes, with each ' written as '\''.
function Exec.quote(path)
	if WIN_ENV then return '"' .. path .. '"' end
	return "'" .. path:gsub("'", "'\\''") .. "'"
end

-- What `LrTasks.execute`'s `status` means, or nil if the app succeeded. On macOS it's the
-- raw wait status: the exit code times 256, or the signal that stopped the app.
local function failure(status)
	if status == 0 then return nil end
	if WIN_ENV then return 'exited with code ' .. tostring(status) end
	local signal = status % 256
	if signal ~= 0 then return 'was stopped by signal ' .. (signal % 128) end
	return 'exited with code ' .. math.floor(status / 256)
end

-- Runs `app` ('klassifier' or 'klassifier-ui') from the plugin folder with `args` (paths
-- quoted with Exec.quote). Returns its output (stdout and stderr), or nil plus an error message.
function Exec.run(app, args)
	local exe = LrPathUtils.child(_PLUGIN.path, assert(APPS[app], app))
	local outFile = Exec.tempFile('klassifier-output.txt')

	local cmd = string.format('%s %s > %s 2>&1', Exec.quote(exe), args, Exec.quote(outFile))
	-- cmd /c needs the whole command wrapped in quotes when it contains quoted paths.
	if WIN_ENV then cmd = '"' .. cmd .. '"' end

	logger:info('Running: ' .. cmd)
	local status = LrTasks.execute(cmd)
	local output = LrFileUtils.readFile(outFile)
	LrFileUtils.delete(outFile)
	local failed = failure(status)
	if failed then
		-- The app prints "error: ..." explaining why; show the end of its output.
		local detail = output and output:gsub('%s+$', ''):sub(-1000) or ''
		return nil, app .. ' ' .. failed .. '\n' .. detail
	end
	if not output then
		return nil, 'Could not read output file ' .. outFile
	end
	return output
end

function Exec.splitTabs(line)
	local fields = {}
	for field in (line .. '\t'):gmatch('([^\t]*)\t') do
		fields[#fields + 1] = field
	end
	return fields
end

-- `s` as one field: tabs and line breaks become spaces.
function Exec.clean(s)
	return (tostring(s or ''):gsub('[\t\r\n]', ' '))
end

-- `s` as one field that keeps its line breaks: "\" -> "\\", line break -> "\n".
function Exec.escape(s)
	return (tostring(s or ''):gsub('\r', ''):gsub('\\', '\\\\'):gsub('\n', '\\n'):gsub('\t', ' '))
end

-- Undoes `escape`: "\\n" -> line break, "\\\\" -> "\\"; any other backslash is kept.
function Exec.unescape(s)
	return (tostring(s or ''):gsub('\\(.?)', function(c)
		if c == 'n' then return '\n' end
		if c == '\\' then return '\\' end
		return '\\' .. c
	end))
end

-- Where each window was last closed is kept in the plugin's preferences, under a key per
-- window, as klassifier-ui's WINDOW line fields: "x<TAB>y<TAB>width<TAB>height<TAB>maximized".
local WINDOW_FIELDS = '^%-?%d+\t%-?%d+\t%d+\t%d+\t[01]$'

-- Adds the WINDOW line for the window saved under `key` to `lines`, if there is one.
function Exec.addSavedWindow(lines, key)
	local saved = LrPrefs.prefsForPlugin()[key]
	if type(saved) == 'string' and saved:match(WINDOW_FIELDS) then lines[#lines + 1] = 'WINDOW\t' .. saved end
end

-- Saves where the window was closed (the WINDOW line in its `output`) under `key`.
function Exec.saveWindow(output, key)
	for line in output:gmatch('[^\r\n]+') do
		local fields = line:match('^WINDOW\t(.*)$')
		if fields and fields:match(WINDOW_FIELDS) then LrPrefs.prefsForPlugin()[key] = fields end
	end
end

-- Writes `lines` to `file`, one per line. Returns true, or nil plus an error message.
function Exec.writeLines(file, lines)
	local f, err = io.open(file, 'wb')
	if not f then return nil, err end
	f:write(table.concat(lines, '\n'), '\n')
	f:close()
	return true
end

return Exec
