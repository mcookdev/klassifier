# Hello World Lightroom Plugin

A minimal Lightroom Classic plug-in that adds a "Say Hello" command under
**File > Plug-in Extras** and **Library > Plug-in Extras**. It shows a
greeting dialog with the number of currently selected photos.

## Requirements

- Adobe Lightroom Classic (SDK 6.0 or later, i.e. Lightroom 6 / CC Classic and newer)

## Installation

1. Copy (or clone) the `HelloWorld.lrplugin` folder to a location on disk.
   Lightroom reads the plugin directly from this folder, so keep it
   somewhere permanent (it doesn't need to be copied into the Lightroom
   app directory).

2. Open Lightroom Classic.

3. Go to **File > Plug-in Manager**.

4. Click **Add** in the bottom-left corner.

5. Browse to and select the `HelloWorld.lrplugin` folder, then click
   **Add Plug-in**.

6. Confirm the plugin shows up in the list as "Hello World" with status
   "Installed and running". Click **Done** to close the manager.

## Usage

- Select one or more photos (optional).
- Go to **File > Plug-in Extras > Say Hello** (or **Library > Plug-in
  Extras > Say Hello**).
- A dialog appears greeting you and reporting how many photos are
  targeted.

## Updating the plugin

If you edit the plugin's Lua files while Lightroom is running, use
**File > Plug-in Manager > Reload Plug-in** (select the plugin first)
to pick up changes without restarting Lightroom.

## Logs

The plugin writes to `HelloWorld.log` in Lightroom's logs folder when
"Say Hello" is invoked, useful for debugging via **File > Plug-in
Manager**'s log file link or the folder shown there.

## Uninstalling

Open **File > Plug-in Manager**, select "Hello World", and click
**Remove**. You can then delete the `HelloWorld.lrplugin` folder from
disk if desired.
