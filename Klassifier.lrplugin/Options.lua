--[[----------------------------------------------------------------------------
  Options.lua
  The Classify Photos options: which label set, extra hints, and whether to use
  only the hints. klassifier-ui --options asks for them (format in
  klassifier/src/ui_model.rs); the choices are remembered in the plugin's
  preferences.
------------------------------------------------------------------------------]]

local LrDialogs   = import 'LrDialogs'
local LrFileUtils = import 'LrFileUtils'
local LrLogger    = import 'LrLogger'
local LrPathUtils = import 'LrPathUtils'
local LrPrefs     = import 'LrPrefs'

local Exec = require 'Exec'

local logger = LrLogger('Klassifier')
logger:enable('logfile')

local Options = {}

-- Warn in the options dialog above this many photos (e.g. a whole filmstrip with nothing selected).
local WARN_ABOVE = 500

-- Label sets are the .txt files in two folders: the plugin's own, replaced with the plugin on
-- every update, and the user's, which updates leave alone. A user set replaces a built-in one
-- of the same name. klassifier-ui lists them (list_label_sets in ui_model.rs).
local USER_LABEL_SET_DIR = 'Klassifier Label Sets'

function Options.builtinLabelSetDir()
	return LrPathUtils.child(_PLUGIN.path, 'label-sets')
end

-- In Lightroom's own app data folder (on macOS ~/Library/Application Support/Adobe/Lightroom).
function Options.userLabelSetDir()
	return LrPathUtils.child(LrPathUtils.getStandardFilePath('appData'), USER_LABEL_SET_DIR)
end

-- Whether `path` is a file directly in `dir` (ignoring case on Windows, whose paths do).
local function isFileIn(path, dir)
	local parent = LrPathUtils.parent(path)
	if WIN_ENV then parent, dir = parent and parent:lower(), dir:lower() end
	return parent == dir and LrFileUtils.exists(path) == 'file'
end

-- "makeup, hair styling,,backstage" -> { 'makeup', 'hair styling', 'backstage' }
function Options.parseHints(text)
	local hints = {}
	for hint in (text or ''):gmatch('[^,;\r\n]+') do
		hint = hint:match('^%s*(.-)%s*$')
		if hint ~= '' then hints[#hints + 1] = hint end
	end
	return hints
end

-- Shows the options dialog (klassifier-ui --options; format in ui_model.rs). Returns
-- { labelSet, labelFile, hints, onlyHints }, or nil if cancelled. The choices are remembered for next time.
function Options.ask(context, photoCount)
	local prefs = LrPrefs.prefsForPlugin()
	local builtinDir, userDir = Options.builtinLabelSetDir(), Options.userLabelSetDir()
	-- Made now so the folder button always has a folder to open. Not fatal: the button makes it too.
	if not LrFileUtils.exists(userDir) and not LrFileUtils.createAllDirectories(userDir) then
		logger:warn('Could not create ' .. userDir)
	end

	local input = { '#klassifier-ui\toptions\t2' }
	Exec.addSavedWindow(input, 'optionsWindow')
	input[#input + 1] = 'COUNT\t' .. photoCount
	input[#input + 1] = 'WARN_ABOVE\t' .. WARN_ABOVE
	input[#input + 1] = 'SETDIR\tbuiltin\t' .. Exec.clean(builtinDir)
	input[#input + 1] = 'SETDIR\tuser\t' .. Exec.clean(userDir)
	if prefs.labelSet then input[#input + 1] = 'LABELSET\t' .. Exec.clean(prefs.labelSet) end
	input[#input + 1] = 'HINTS\t' .. Exec.escape(prefs.hints or '')
	input[#input + 1] = 'ONLYHINTS\t' .. (prefs.onlyHints and 1 or 0)

	local inFile = Exec.tempFile('klassifier-options.txt')
	context:addCleanupHandler(function() LrFileUtils.delete(inFile) end)
	local ok, err = Exec.writeLines(inFile, input)
	local output, runErr
	if ok then
		output, runErr = Exec.run('klassifier-ui', '--options --in ' .. Exec.quote(inFile))
	end
	if not output then
		local why = runErr or ('Could not write ' .. inFile .. ': ' .. tostring(err))
		logger:error(why)
		LrDialogs.message('Classify Photos', 'The options window failed:\n' .. why, 'critical')
		return nil
	end
	Exec.saveWindow(output, 'optionsWindow')

	local result, chosen = nil, {}
	for line in output:gmatch('[^\r\n]+') do
		local fields = Exec.splitTabs(line)
		if fields[1] == 'RESULT' then result = fields[2]
		elseif fields[1] == 'LABELSET' then chosen.labelSet = fields[2]
		elseif fields[1] == 'LABELFILE' then chosen.labelFile = fields[2]
		elseif fields[1] == 'HINTS' then chosen.hints = Exec.unescape(fields[2])
		elseif fields[1] == 'ONLYHINTS' then chosen.onlyHints = fields[2] == '1'
		end
	end
	if not result then
		local why = 'No result from klassifier-ui:\n' .. output:sub(-1000)
		logger:error(why)
		LrDialogs.message('Classify Photos', 'The options window failed:\n' .. why, 'critical')
		return nil
	end
	if result ~= 'ok' then return nil end
	-- The set's file must be one of the label sets, unless only the hints are used.
	local file = chosen.labelFile
	local fileOk = file and (isFileIn(file, builtinDir) or isFileIn(file, userDir))
	if chosen.hints == nil or chosen.onlyHints == nil or not (fileOk or chosen.onlyHints) then
		logger:error('Unexpected options output:\n' .. output)
		LrDialogs.message('Classify Photos', 'The options window gave an unexpected answer:\n' .. output:sub(1, 1000), 'critical')
		return nil
	end

	prefs.labelSet, prefs.hints, prefs.onlyHints = chosen.labelSet, chosen.hints, chosen.onlyHints
	-- The window checks these too; kept as a backstop.
	local options = {
		labelSet = chosen.labelSet, labelFile = file, hints = Options.parseHints(chosen.hints), onlyHints = chosen.onlyHints,
	}
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

return Options
