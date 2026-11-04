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

	-- Adds "Hello World" under File > Plug-in Extras
	LrExportMenuItems = {
		{
			title = "Say Hello",
			file = "HelloWorld.lua",
		},
	},

	-- Adds "Hello World" under Library > Plug-in Extras
	LrLibraryMenuItems = {
		{
			title = "Say Hello",
			file = "HelloWorld.lua",
		},
	},

	VERSION = { major = 1, minor = 1, revision = 0, build = 2 },
}
