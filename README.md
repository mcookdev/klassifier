# Hello World Lightroom Plugin

A Lightroom Classic plug-in that adds a "Say Hello" command under
**File > Plug-in Extras** and **Library > Plug-in Extras**. It runs a small
Windows console app (`hello-app.exe`, written in Rust) on the targeted photos.
The app reads a preview image for each photo, and the plugin shows a report of
what it read. This is the groundwork for classifying what's in each photo.

For RAW files the app uses the JPEG preview embedded in the file and never
decodes the raw data. JPEG, PNG, TIFF and WebP files are read directly. Other
formats (HEIC, PSD, ...) are reported as unsupported. Images are rotated
upright using their EXIF orientation.

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

## Testing

```bash
make test
```

This runs the app's tests natively on Linux. The tests that use real photos
read `test-data/manifest.tsv` at the repo root and are skipped if it's
missing. `test-data/` is gitignored because it holds personal photos. Each
manifest line is tab-separated:

```
<file name><TAB><source><TAB><min long edge><TAB>portrait|landscape
```

where `source` is the value the app reports (`file`, `preview`, `full` or
`thumbnail`). Lines starting with `#` are comments.

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
- If nothing is selected, Lightroom targets the whole filmstrip. The plugin
  asks before processing more than 500 photos.
- Videos are skipped. Virtual copies that share a file with another targeted
  photo are processed only once.
- Photos go to `hello-app.exe` 100 at a time, with a progress bar. Cancel
  takes effect between batches. A console window may flash for each batch.
- The report dialog lists each photo's preview size, where it came from and
  any rotation. It marks previews under 224 px as `[SMALL]` and shows errors.

### Running the app directly

```bash
hello-app.exe [--list <file>] [--dump <dir>] [paths...]
```

- `--list <file>` reads photo paths from a UTF-8 file, one per line.
- `--dump <dir>` saves each loaded image, at most 1024 px, as
  `<dir>/<file name>.jpg` so you can check it by eye.

The output is tab-separated, one line per photo (format documented in
`hello-app/src/output.rs`):

```
#hello-app	1.2.0
OK	D:\Photos\DSC01674.ARW	preview	1616	1080	0	5
ERR	D:\Photos\IMG_0001.HEIC	unsupported: .heic
#summary	ok=1	err=1	ms=12
```

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
