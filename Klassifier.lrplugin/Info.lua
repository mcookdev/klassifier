--[[----------------------------------------------------------------------------
  Info.lua
  Klassifier plugin for Lightroom Classic.
------------------------------------------------------------------------------]]

return {
	LrSdkVersion = 6.0,
	LrSdkMinimumVersion = 6.0,

	LrToolkitIdentifier = 'io.github.mcookdev.klassifier',
	LrPluginName = "Klassifier",
	LrPluginInfoUrl = "https://github.com/mcookdev/klassifier",

	-- Adds "Classify Photos" under File > Plug-in Extras
	LrExportMenuItems = {
		{
			title = "Classify Photos",
			file = "Klassifier.lua",
		},
	},

	-- Adds "Classify Photos" under Library > Plug-in Extras
	LrLibraryMenuItems = {
		{
			title = "Classify Photos",
			file = "Klassifier.lua",
		},
	},

	VERSION = { major = 3, minor = 2, revision = 0, build = 64 },
}
