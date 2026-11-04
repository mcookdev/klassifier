# Hello World Lightroom Plugin

A Lightroom Classic plug-in that adds a "Classify Photos" command under
**File > Plug-in Extras** and **Library > Plug-in Extras**. It runs a small
Windows console app (`hello-app.exe`, written in Rust) on the targeted photos.
The app scores each photo against a list of labels ("dog", "beach",
"sunset", ...) with the [SigLIP](https://huggingface.co/google/siglip-base-patch16-224)
image model, and the plugin shows the best matches in a dialog. Nothing is
written to the catalog.

The model runs on the GPU through DirectML (any DirectX 12 GPU) and falls back
to the CPU. This small model is only slightly slower on the CPU.

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

Download the runtime files, about 830 MB, into `vendor/` (gitignored):

```bash
make fetch
```

This gets ONNX Runtime 1.24.4 and DirectML 1.15.4 from NuGet, and the SigLIP
model (`Xenova/siglip-base-patch16-224`, ONNX export) from Hugging Face.
`make deploy` runs it too, and it skips files that are already downloaded.

Build and deploy:

```bash
make deploy
```

Or press **Ctrl+Shift+B** in VS Code. This cross-compiles `hello-app.exe` and
copies the plugin folder, the exe and the runtime files to
`C:\Users\cadre\lr-plugins\HelloWorld.lrplugin`. Only changed files are
copied, so the model is copied once. To deploy somewhere else:

```bash
make deploy DEPLOY_DIR=/mnt/c/somewhere
```

## Testing

```bash
make test
```

This runs the app's tests natively on Linux. They cover reading previews, not
the classifier, because ONNX Runtime is only deployed for Windows. The tests that use real photos
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
   Don't add the repo's `HelloWorld.lrplugin` folder; it doesn't contain the exe
   or the model.
4. Confirm the plugin shows up as "Hello World" with status
   "Installed and running". Click **Done**.

## Usage

- Select one or more photos (optional).
- Go to **File > Plug-in Extras > Classify Photos** (or **Library > Plug-in
  Extras > Classify Photos**).
- If nothing is selected, Lightroom targets the whole filmstrip. The plugin
  asks before processing more than 500 photos.
- Videos are skipped. Virtual copies that share a file with another targeted
  photo are processed only once.
- Photos go to `hello-app.exe` 100 at a time, with a progress bar. Cancel
  takes effect between batches. A console window may flash for each batch.
  Each batch spends about half a second loading the model, then roughly
  30 ms per ARW and 60-120 ms per CR2 or JPEG.
- The report dialog lists each photo's match level and its top 5 labels. It
  also shows the preview size and where it came from, and errors.

### Reading the report

```
IMG_7925.CR2  -  strong match    (3456x5184 preview, 116 ms)
        halloween 93%, costume 7%, child 1%, baby <1%, holiday <1%
```

- The percentages are each label's share of the photo's total score across
  all labels. They show how the labels compare for this photo.
- Shares always add up to 100%, so some label gets a big share even when
  nothing fits. The match level shows how sure the model is about the top
  label, from its raw score:
  - **strong:** raw score of 10% or more
  - **good:** 1% to 10%
  - **weak:** under 1%, so treat the labels as guesses

The raw score is SigLIP's own estimate that a label applies, judged on its
own. It's low even for clear matches (often 5-30%). It's in the app's output
and is what a keyword cutoff should use, since it means the same thing in
every photo.

### Labels

`HelloWorld.lrplugin/labels.txt` lists the labels, one per line. Lines
starting with `#` are comments. Each label becomes the prompt
"a photo of <label>". Concrete nouns ("dog", "bridge", "birthday party") work
better than abstract ideas. After editing, run `make deploy`. The first
classification after a change computes the label embeddings with the text
model (a second or two) and caches them in `%TEMP%`.

### How RAW files are read

For RAW files the app uses the JPEG preview embedded in the file and never
decodes the raw data. That preview shows the in-camera look, not your
Lightroom edits. JPEG, PNG, TIFF and WebP files are read directly. Other
formats (HEIC, PSD, ...) are reported as unsupported. Images are rotated
upright using their EXIF orientation.

### Running the app directly

```bash
hello-app.exe [options] [paths...]
```

- `--list <file>` reads photo paths from a UTF-8 file, one per line.
- `--top <n>` reports the `n` best labels per photo (default 5).
- `--labels <file>` uses another labels file instead of `labels.txt` next to
  the exe.
- `--cpu` doesn't use the GPU.
- `--no-classify` only reads the previews. It needs no model files.
- `--dump <dir>` saves each loaded image, at most 1024 px, as
  `<dir>/<file name>.jpg` so you can check it by eye.

The output is tab-separated, one line per photo (format documented in
`hello-app/src/output.rs`):

```
#hello-app	1.4.0	backend=DirectML
#model	load_ms=520	labels=98	label_cache=hit
OK	D:\Photos\DSC09840.dng	preview	683	1024	270	22		forest=0.1589;tree=0.1422;landscape=0.0235;river=0.0147;lake=0.0074	forest=0.4512;tree=0.3628;landscape=0.0681;river=0.0418;lake=0.0306	strong
ERR	D:\Photos\IMG_0001.HEIC	unsupported: .heic
#summary	ok=1	err=1	ms=40
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
