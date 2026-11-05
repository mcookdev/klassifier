--[[----------------------------------------------------------------------------
  Info.lua
  Hello World plugin for Lightroom Classic.
------------------------------------------------------------------------------]]

return {
	LrSdkVersion = 6.0,
	LrSdkMinimumVersion = 6.0,

	LrToolkitIdentifier = 'com.example.lightroom.helloworld',
	LrPluginName = "Hello World",
	LrPluginInfoUrl = "https://www.example.com",

	-- Adds "Classify Photos" under File > Plug-in Extras
	LrExportMenuItems = {
		{
			title = "Classify Photos",
			file = "HelloWorld.lua",
		},
	},

	-- Adds "Classify Photos" under Library > Plug-in Extras
	LrLibraryMenuItems = {
		{
			title = "Classify Photos",
			file = "HelloWorld.lua",
		},
	},

	VERSION = { major = 1, minor = 12, revision = 0, build = 13 },
}
