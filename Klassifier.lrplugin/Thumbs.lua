--[[----------------------------------------------------------------------------
  Thumbs.lua
  Thumbnails for the review window, in a temp folder:
    <n>.jpg     stand-in, written by klassifier --thumbs while classifying, from
                the image it reads (the camera's rendering of a RAW file)
    <n>.lr.jpg  Lightroom's rendering with the photo's edits, requested here in the
                background. Fast when Lightroom has previews; for photos without, about
                0.4 s each, one at a time, so it can arrive long after the stand-in.
  <n> is the photo's position in the list classified. The review window shows the
  best file there and picks up <n>.lr.jpg when it appears.
------------------------------------------------------------------------------]]

local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'
local LrPathUtils = import 'LrPathUtils'
local LrTasks     = import 'LrTasks'

local Exec = require 'Exec'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Thumbs = {}

-- Long edge requested from Lightroom, in pixels: room for the grid on high-DPI screens.
local SIZE = 640
-- Requests in flight. Lightroom renders them one at a time, so a few is enough.
local MAX_PENDING = 4

-- Starts requesting Lightroom's thumbnail of each entry's first photo (entries as from
-- collectTargets in Klassifier.lua). Returns { dir, standin(n), rendered(n) }. The folder
-- and any requests still running are dropped when `context` ends.
function Thumbs.start(context, entries)
	local dir = Exec.tempFile('klassifier-thumbs')
	LrFileUtils.createAllDirectories(dir)
	local thumbs = { dir = dir }
	function thumbs.standin(n) return LrPathUtils.child(dir, n .. '.jpg') end
	function thumbs.rendered(n) return LrPathUtils.child(dir, n .. '.lr.jpg') end

	local stopped, pending = false, 0
	-- Each request's returned object is kept until it answers: Lightroom may cancel a
	-- request whose object is garbage collected.
	local requests = {}
	context:addCleanupHandler(function()
		stopped = true
		LrFileUtils.delete(dir)
	end)

	LrTasks.startAsyncTask(function()
		for n, entry in ipairs(entries) do
			while pending >= MAX_PENDING and not stopped do LrTasks.sleep(0.02) end
			if stopped then return end
			pending = pending + 1
			local answered = false
			requests[n] = entry.photos[1]:requestJpegThumbnail(SIZE, SIZE, function(data)
				-- Only the first answer counts.
				if answered then return end
				answered = true
				pending = pending - 1
				requests[n] = nil
				if stopped or not data then return end
				-- Written under another name first, so the window never reads half a file.
				local part = thumbs.rendered(n) .. '.part'
				local f = io.open(part, 'wb')
				if not f then return end
				f:write(data)
				f:close()
				LrFileUtils.move(part, thumbs.rendered(n))
				if not LrFileUtils.exists(thumbs.rendered(n)) then
					logger:warn('Could not save Lightroom\'s thumbnail ' .. thumbs.rendered(n))
				end
			end)
		end
	end)
	return thumbs
end

return Thumbs
