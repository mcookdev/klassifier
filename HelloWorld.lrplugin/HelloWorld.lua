--[[----------------------------------------------------------------------------
  HelloWorld.lua
  Shows a greeting and the number of photos currently selected.
------------------------------------------------------------------------------]]

local LrApplication = import 'LrApplication'
local LrDialogs     = import 'LrDialogs'
local LrTasks       = import 'LrTasks'
local LrLogger      = import 'LrLogger'

local logger = LrLogger('HelloWorld')
logger:enable('logfile') -- writes to HelloWorld.log in the Lightroom logs folder

-- Catalog access must happen inside an async task.
LrTasks.startAsyncTask(function()
	local catalog = LrApplication.activeCatalog()
	local photos = catalog:getTargetPhotos()
	local count = #photos

	logger:info('Say Hello invoked with ' .. count .. ' photo(s) selected')

	LrDialogs.message(
		"Hello, World!",
		-- getTargetPhotos() returns the selection, or the whole filmstrip if nothing is selected.
		string.format("Hello from Lightroom Classic!\n%d photo(s) targeted.", count),
		"info"
	)
end)
