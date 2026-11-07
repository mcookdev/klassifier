--[[----------------------------------------------------------------------------
  Info.lua
  Klassifier plugin for Lightroom Classic.
------------------------------------------------------------------------------]]

return {
	LrSdkVersion = 6.0,
	LrSdkMinimumVersion = 6.0,

	LrToolkitIdentifier = 'com.example.lightroom.klassifier',
	LrPluginName = "Klassifier",
	LrPluginInfoUrl = "https://www.example.com",

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

	VERSION = { major = 2, minor = 8, revision = 0, build = 29 },
}
