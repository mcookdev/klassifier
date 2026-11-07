--[[----------------------------------------------------------------------------
  Exec.lua
  Runs the plugin's exes (klassifier.exe, klassifier-ui.exe) and helps read and
  write the tab-separated lines they exchange with the plugin.
------------------------------------------------------------------------------]]

local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'
local LrPathUtils = import 'LrPathUtils'
local LrTasks     = import 'LrTasks'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Exec = {}

-- True while Classify Photos runs (see Klassifier.lua).
Exec.running = false

-- A new file name in the temp folder, based on `name`.
function Exec.tempFile(name)
	return LrFileUtils.chooseUniqueFileName(LrPathUtils.child(LrPathUtils.getStandardFilePath('temp'), name))
end

-- Runs `exeName` from the plugin folder with `args` (already quoted). Returns its output
-- (stdout and stderr), or nil plus an error message.
function Exec.run(exeName, args)
	local exe = LrPathUtils.child(_PLUGIN.path, exeName)
	local outFile = Exec.tempFile('klassifier-output.txt')

	local cmd = string.format('"%s" %s > "%s" 2>&1', exe, args, outFile)
	-- cmd /c needs the whole command wrapped in quotes when it contains quoted paths.
	if WIN_ENV then cmd = '"' .. cmd .. '"' end

	logger:info('Running: ' .. cmd)
	local exitCode = LrTasks.execute(cmd)
	local output = LrFileUtils.readFile(outFile)
	LrFileUtils.delete(outFile)
	if exitCode ~= 0 then
		-- The exe prints "error: ..." explaining why; show the end of its output.
		local detail = output and output:gsub('%s+$', ''):sub(-1000) or ''
		return nil, exeName .. ' exited with code ' .. tostring(exitCode) .. '\n' .. detail
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

-- Writes `lines` to `file`, one per line. Returns true, or nil plus an error message.
function Exec.writeLines(file, lines)
	local f, err = io.open(file, 'wb')
	if not f then return nil, err end
	f:write(table.concat(lines, '\n'), '\n')
	f:close()
	return true
end

return Exec
