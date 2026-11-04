# Hello World Lightroom Plugin

A minimal Lightroom Classic plug-in that adds a "Say Hello" command under
**File > Plug-in Extras** and **Library > Plug-in Extras**. It runs a small
Windows console app (`hello-app.exe`, written in Rust) and shows the app's
output in a dialog, along with the number of currently selected photos.

## Requirements

- Adobe Lightroom Classic (SDK 6.0 or later) on Windows
- WSL with Rust (`cargo`) and `make` for building

## Building

One-time setup (in WSL):

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt update && sudo apt install -y gcc-mingw-w64-x86-64
```

Build and deploy:

```bash
make deploy
```

Or press **Ctrl+Shift+B** in VS Code. This cross-compiles `hello-app.exe` and
copies the plugin folder, with the exe inside it, to
`C:\Users\cadre\lr-plugins\HelloWorld.lrplugin`. To deploy somewhere else:

```bash
make deploy DEPLOY_DIR=/mnt/c/somewhere
```

## Installation

1. Run `make deploy`.
2. Open Lightroom Classic and go to **File > Plug-in Manager**.
3. Click **Add** and select `C:\Users\cadre\lr-plugins\HelloWorld.lrplugin`.
   Don't add the repo's `HelloWorld.lrplugin` folder; it doesn't contain the exe.
4. Confirm the plugin shows up as "Hello World" with status
   "Installed and running". Click **Done**.

## Usage

- Select one or more photos (optional).
- Go to **File > Plug-in Extras > Say Hello** (or **Library > Plug-in
  Extras > Say Hello**).
- A dialog shows the message returned by `hello-app.exe` and how many photos
  are targeted. A console window may flash briefly while the app runs.

## Updating the plugin

Run `make deploy`, then in **File > Plug-in Manager** select the plugin and
click **Reload Plug-in**.

## Logs

The plugin writes to `HelloWorld.log` in Lightroom's logs folder, including
the exact command line used to run `hello-app.exe`. Open it from the
Plug-in Manager's log file link.

## Uninstalling

Open **File > Plug-in Manager**, select "Hello World", and click
**Remove**. You can then delete `C:\Users\cadre\lr-plugins\HelloWorld.lrplugin`.
