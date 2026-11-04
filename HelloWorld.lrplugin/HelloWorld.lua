--[[----------------------------------------------------------------------------
  HelloWorld.lua
  Runs hello-app.exe and shows its output plus the number of selected photos.
------------------------------------------------------------------------------]]

local LrApplication = import 'LrApplication'
local LrDialogs     = import 'LrDialogs'
local LrFileUtils   = import 'LrFileUtils'
local LrLogger      = import 'LrLogger'
local LrPathUtils   = import 'LrPathUtils'
local LrTasks       = import 'LrTasks'

local logger = LrLogger('HelloWorld')
logger:enable('logfile') -- writes to HelloWorld.log in the Lightroom logs folder

-- Returns the app's trimmed stdout, or nil plus an error message.
local function runHelloApp()
	local exe = LrPathUtils.child(_PLUGIN.path, 'hello-app.exe')
	local outFile = LrPathUtils.child(LrPathUtils.getStandardFilePath('temp'), 'hello-app-output.txt')

	local cmd = string.format('"%s" > "%s"', exe, outFile)
	-- cmd /c needs the whole command wrapped in quotes when it contains quoted paths.
	if WIN_ENV then cmd = '"' .. cmd .. '"' end

	logger:info('Running: ' .. cmd)
	local exitCode = LrTasks.execute(cmd)
	if exitCode ~= 0 then
		return nil, 'hello-app.exe exited with code ' .. tostring(exitCode)
	end

	local output = LrFileUtils.readFile(outFile)
	LrFileUtils.delete(outFile)
	if not output then
		return nil, 'Could not read output file ' .. outFile
	end
	return (output:gsub('%s+$', ''))
end

-- Catalog access must happen inside an async task.
LrTasks.startAsyncTask(function()
	local catalog = LrApplication.activeCatalog()
	local count = #catalog:getTargetPhotos()

	local greeting, err = runHelloApp()
	if not greeting then
		logger:error(err)
		LrDialogs.message("Hello, World!", "Failed to run hello-app.exe:\n" .. err, "critical")
		return
	end

	logger:info('hello-app returned: ' .. greeting)
	LrDialogs.message(
		"Hello, World!",
		-- getTargetPhotos() returns the selection, or the whole filmstrip if nothing is selected.
		string.format("%s\n%d photo(s) targeted.", greeting, count),
		"info"
	)
end)
