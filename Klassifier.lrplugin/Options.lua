--[[----------------------------------------------------------------------------
  Options.lua
  The Classify Photos options: which label set, extra hints, and whether to use
  only the hints. klassifier-ui.exe --options asks for them (format in
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

-- Label sets are the .txt files in this folder of the plugin.
Options.LABEL_SET_DIR = 'label-sets'
local DEFAULT_LABEL_SET = 'General'

-- Names of the label sets (the .txt files in Options.LABEL_SET_DIR), the default first.
function Options.listLabelSets()
	local names = {}
	for file in LrFileUtils.files(LrPathUtils.child(_PLUGIN.path, Options.LABEL_SET_DIR)) do
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

-- "makeup, hair styling,,backstage" -> { 'makeup', 'hair styling', 'backstage' }
function Options.parseHints(text)
	local hints = {}
	for hint in (text or ''):gmatch('[^,;\r\n]+') do
		hint = hint:match('^%s*(.-)%s*$')
		if hint ~= '' then hints[#hints + 1] = hint end
	end
	return hints
end

-- Shows the options dialog (klassifier-ui.exe --options; format in ui_model.rs). Returns
-- { labelSet, hints, onlyHints }, or nil if cancelled. The choices are remembered for next time.
function Options.ask(context, photoCount)
	local prefs = LrPrefs.prefsForPlugin()
	local sets = Options.listLabelSets()
	if #sets == 0 then
		LrDialogs.message('Classify Photos', 'No label sets found in the plugin\'s ' .. Options.LABEL_SET_DIR .. ' folder.', 'critical')
		return nil
	end

	local input = { '#klassifier-ui\toptions\t1' }
	Exec.addSavedWindow(input, 'optionsWindow')
	input[#input + 1] = 'COUNT\t' .. photoCount
	input[#input + 1] = 'WARN_ABOVE\t' .. WARN_ABOVE
	for _, name in ipairs(sets) do input[#input + 1] = 'SET\t' .. Exec.clean(name) end
	if prefs.labelSet then input[#input + 1] = 'LABELSET\t' .. Exec.clean(prefs.labelSet) end
	input[#input + 1] = 'HINTS\t' .. Exec.escape(prefs.hints or '')
	input[#input + 1] = 'ONLYHINTS\t' .. (prefs.onlyHints and 1 or 0)

	local inFile = Exec.tempFile('klassifier-options.txt')
	context:addCleanupHandler(function() LrFileUtils.delete(inFile) end)
	local ok, err = Exec.writeLines(inFile, input)
	local output, runErr
	if ok then
		output, runErr = Exec.run('klassifier-ui.exe', string.format('--options --in "%s"', inFile))
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
		elseif fields[1] == 'HINTS' then chosen.hints = Exec.unescape(fields[2])
		elseif fields[1] == 'ONLYHINTS' then chosen.onlyHints = fields[2] == '1'
		end
	end
	if not result then
		local why = 'No result from klassifier-ui.exe:\n' .. output:sub(-1000)
		logger:error(why)
		LrDialogs.message('Classify Photos', 'The options window failed:\n' .. why, 'critical')
		return nil
	end
	if result ~= 'ok' then return nil end
	local known = false
	for _, name in ipairs(sets) do known = known or name == chosen.labelSet end
	if not known or chosen.hints == nil or chosen.onlyHints == nil then
		logger:error('Unexpected options output:\n' .. output)
		LrDialogs.message('Classify Photos', 'The options window gave an unexpected answer:\n' .. output:sub(1, 1000), 'critical')
		return nil
	end

	prefs.labelSet, prefs.hints, prefs.onlyHints = chosen.labelSet, chosen.hints, chosen.onlyHints
	-- The window checks these too; kept as a backstop.
	local options = { labelSet = chosen.labelSet, hints = Options.parseHints(chosen.hints), onlyHints = chosen.onlyHints }
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
