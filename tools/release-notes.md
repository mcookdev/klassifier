<!-- The notes .github/workflows/release.yml puts on each release, before the commits since the
last one. It fills in @-placeholders: the version, the build, the commit and the Build run. -->
Klassifier @VERSION@, build @BUILD@: [built by this run](@RUN_URL@) from @SHA@.

## Installing

Download the zip for your platform below. Each one is the whole plugin: the apps, ONNX Runtime and the SigLIP model.

**Windows**

1. Right-click the zip, choose **Properties**, tick **Unblock** and click **OK**, so Windows doesn't block the files inside.
2. Extract it to a folder of your own, such as `C:\Users\<you>\lr-plugins`.
3. In Lightroom Classic, go to **File > Plug-in Manager**, click **Add** and select the extracted `Klassifier.lrplugin` folder.

**macOS (Apple Silicon, macOS 14 or later)**

1. Unzip it and move `Klassifier.lrplugin` to `~/Library/Application Support/Adobe/Lightroom/Modules`.
2. The app isn't notarized, so clear the flag macOS puts on downloads:
   `xattr -dr com.apple.quarantine ~/Library/Application\ Support/Adobe/Lightroom/Modules/Klassifier.lrplugin`
3. Restart Lightroom Classic. It loads the plugin from that folder at startup.

To update, replace the old `Klassifier.lrplugin` folder with the new one.
