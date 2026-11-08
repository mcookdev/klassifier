# Klassifier

A Lightroom Classic plug-in for Windows and macOS (Apple Silicon) that adds a
"Classify Photos" command under **File > Plug-in Extras** and **Library >
Plug-in Extras**. It runs a small console app (`klassifier`, written in Rust;
`klassifier.exe` on Windows) on the targeted photos. The app scores each photo
against a list of labels ("dog", "beach", "sunset", ...) with the
[SigLIP](https://huggingface.co/google/siglip-base-patch16-224) image model, and
the plugin shows the suggested keywords for each photo in a review window. The
plugin's windows (the options dialog, the review and its report) are
`klassifier-ui`, also Rust, built with [Slint](https://slint.dev); on macOS
it's in `Klassifier.app`, so it shows as Klassifier in the Dock. When you
accept the keywords, the plugin adds them to the photos in the catalog.

The model runs on the GPU and falls back to the CPU:

- **Windows:** DirectML (any DirectX 12 GPU). This small model is only
  slightly slower on the CPU. On a PC with more than one GPU (say a desktop
  CPU's integrated graphics and a graphics card), it uses the one Windows
  ranks as high-performance, normally the graphics card.
- **macOS:** WebGPU, which ONNX Runtime runs on Metal: about twice as fast as
  the CPU on an M3 (60 ms against 125 ms per photo). CoreML isn't used: it
  can run almost none of this model.

`Klassifier.log` names what it ran on (`backend=DirectML gpu=NVIDIA GeForce
RTX 4090`, `backend=WebGPU gpu=Apple M3`), and so does the report shown when
no photo could be classified.

For RAW files the model sees the camera's embedded JPEG preview, not your
Lightroom edits (see [How RAW files are read](#how-raw-files-are-read)).

## Requirements

- Adobe Lightroom Classic (SDK 6.0 or later) on Windows, or on macOS 14
  (Sonoma) or later on an Apple Silicon (M-series) Mac. ONNX Runtime's macOS
  build needs macOS 14.
- For building, on the platform's own machine (each builds for itself):
  - Windows: WSL with Rust (`cargo`), `make`, `curl`, `python3` and `rsync`
    (the last three are usually preinstalled on Ubuntu)
  - macOS: Rust (`cargo`) and the Xcode command line tools
    (`xcode-select --install`: `make`, `cc`, `python3`). macOS comes with
    `curl` and `rsync`.
- Only to change the app icon: `pip install resvg-py pillow` (see
  [The app icon](#the-app-icon))

## Building

`make` builds for the machine it runs on: macOS on a Mac, Windows in WSL. The
shared steps are in `Makefile`, and what differs per platform (the Rust target,
the ONNX Runtime files, what's deployed and where) in `mk/windows.mk` and
`mk/macos.mk`.

One-time setup in WSL:

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt update && sudo apt install -y gcc-mingw-w64-x86-64 lua5.1
```

One-time setup on a Mac:

```bash
rustup target add aarch64-apple-darwin
```

Download the runtime files, about 830 MB, into `vendor/` (gitignored):

```bash
make fetch
```

This gets the SigLIP model (`Xenova/siglip-base-patch16-224`, ONNX export) from
Hugging Face into `vendor/models/`, and ONNX Runtime 1.24.4 into
`vendor/windows/` (with DirectML 1.15.4, from NuGet) or `vendor/macos/` (from
its GitHub release). Each download is checked against a pinned SHA-256.
`make deploy` runs it too, and it skips files that are already downloaded.

Each deploy needs a new version, so bump it first:

```bash
make bump              # bug fixes, docs, build tooling: 2.9.3 -> 2.9.4
make bump PART=minor   # new features: 2.9.3 -> 2.10.0
make bump PART=major   # breaking changes: 2.9.3 -> 3.0.0
```

This sets the version in `Info.lua` and `Cargo.toml` and adds 1 to `build`. `make deploy`
stops unless the two files agree and both the version and `build` are newer than the
version deployed to that folder (see [CLAUDE.md](CLAUDE.md)). Each platform's
deploy folder keeps its own record, so one version can be deployed to both.

Build and deploy:

```bash
make deploy
```

Or press **Ctrl+Shift+B** (**Cmd+Shift+B** on a Mac) in VS Code. This checks
the Lua syntax, runs clippy on the deployed build with warnings as errors
(`make clippy`, so a lint stops the deploy), builds the two apps, assembles the
finished plugin in `dist/Klassifier.lrplugin` (gitignored; `make dist` does just
this step), and copies it to the deploy folder. Only changed files are copied,
so the model is copied once, and `dist/` links to the model in `vendor/` rather
than copying it. Lightroom runs the deployed copy, so it only changes on a
deploy. The deploy folder is:

- Windows: `C:\Users\<you>\lr-plugins\Klassifier.lrplugin`, in your Windows
  user folder, which `make` gets from `cmd.exe`. It's on the Windows drive
  because Lightroom can't run the plugin well from WSL's files.
- macOS: `~/Library/Application Support/Adobe/Lightroom/Modules/Klassifier.lrplugin`,
  which Lightroom loads by itself.

`make reveal` opens the deploy folder in Finder or Explorer.

On macOS the build (`make build`, run by `make deploy`) also wraps
`klassifier-ui` in `Klassifier.app`, with `Info.plist` from
`klassifier/assets/macos/`, the icon and an ad-hoc signature.

To deploy somewhere else:

```bash
make deploy DEPLOY_DIR=/mnt/c/somewhere   # WSL
make deploy DEPLOY_DIR=~/somewhere        # macOS
```

To try the review window without Lightroom:

```bash
make ui-demo
```

This opens it (from WSL, on Windows) with the photos in `test-data/` (repeated up to 500,
`DEMO_COUNT=...` to change) and made-up labels. Its buttons just close it.
`make ui-demo DEMO_ARGS="--screenshot shot.png"` saves a picture of the window
instead of showing it, and `DEMO_PHOTOS=<folder>` uses other photos.

## Installation

On Windows:

1. Run `make deploy`.
2. Open Lightroom Classic and go to **File > Plug-in Manager**.
3. Click **Add** and select `C:\Users\<you>\lr-plugins\Klassifier.lrplugin`
   (`make reveal` opens it). Don't add the repo's `Klassifier.lrplugin` folder;
   it doesn't contain the exe or the model.
4. Confirm the plugin shows up as "Klassifier" with status
   "Installed and running". Click **Done**.

On macOS:

1. Run `make deploy`.
2. Restart Lightroom Classic. It loads plug-ins from the Modules folder at
   startup, so there's nothing to add. Don't **Add** the repo's
   `Klassifier.lrplugin` folder: it has no apps or model, and since both
   copies have the same plugin ID, Lightroom then runs that one and disables
   the deployed one ("klassifier-ui: No such file or directory"). If that
   happened, **Remove** the repo entry in the Plug-in Manager and **Enable**
   the one in the Modules folder.
3. In **File > Plug-in Manager**, confirm "Klassifier" shows as "Installed and
   running", with its path in the Modules folder.

## Usage

- Select one or more photos (optional).
- Go to **File > Plug-in Extras > Classify Photos** (or **Library > Plug-in
  Extras > Classify Photos**).
- A dialog asks for the label set and optional hints (see below), then
  classifies when you click **Classify N photos** (or press Enter). If nothing
  is selected, Lightroom targets the whole filmstrip, so check the count.
  Above 500 photos the dialog warns that it can take a while. Esc cancels. It
  opens centred over Lightroom, or where you last moved it.
- Lightroom stays usable while the dialog is open. If the selection changes
  meanwhile, clicking **Classify** asks whether to classify the photos the
  dialog counted. To classify the new selection, cancel and run Classify Photos
  again.
- Videos are skipped. Virtual copies that share a file with another targeted
  photo are processed only once, and share its suggestions.
- Photos go to `klassifier` 100 at a time, with a progress bar. Cancel
  takes effect between batches. On Windows a console window may flash for
  each batch. Each batch spends about half a second loading the model, then
  roughly 30 ms per ARW and 60-120 ms per CR2 or JPEG on Windows (about
  60 ms per JPEG on an M3 Mac).
- The **Review keywords** window opens where you last left it (maximized if it
  was), or centred over Lightroom the first time, or if that monitor is gone.
  It's moved back onto the screen if part of it would be off. It shows all
  the photos in a scrolling grid that fills the window's width, each with its
  file name and how many keywords it will get ("3 Keywords", or "0 Keywords"
  when nothing was suggested). The top shows the totals, the label set, and
  how many files failed, were unsupported or were skipped. When some failed,
  "N failed" is a link that opens the report below with each photo's error.
- Thumbnails show Lightroom's rendering of each photo, with your edits and
  crops. For photos Lightroom has no previews of yet, rendering takes it about
  0.4 s each, so the grid first shows the image the app read (for RAW files,
  the camera's embedded preview) and swaps in Lightroom's as it arrives.
  Lightroom's rendering is only for display: the model always classifies the
  image the app read.
- The panel on the right shows the selected photo larger, with all its
  labels: the suggested ones are ticked and in bold, and a label mapped to a
  nested keyword shows where it goes ("bird (Wildlife > Bird)"). Click a photo
  to select it, or move with the arrow keys, Home, End, Page Up and Page Down.
- Tick or untick a label by clicking it, or press its number: 1-9 for the
  first nine, 0 for the tenth. The photo then shows "(edited)" and gets exactly
  the ticked labels, even none. The totals update as you go.
- **Save Keywords** (or Enter) under the labels adds the selected photo's ticked
  keywords right away, while the window stays open, so you can check them in
  Lightroom or save photos as you go. The photo then shows "(saved)" in green
  and can't be changed any more. Each save is its own step in **Edit > Undo**.
  If Lightroom's catalog is busy, nothing is added, the panel says why, and you
  can try again. Photos with nothing ticked can't be saved.
- **Save All** (or Ctrl+Enter; Cmd+Enter on macOS) adds the keywords of every photo not saved
  yet: the ticked ones for photos you changed, the suggested ones for the rest. Photos with nothing
  ticked or suggested get nothing. Keywords are only added,
  never removed, and a keyword that already exists is reused, whatever its
  case (`bird` reuses `Bird`). New keywords are top level, named as in the
  label set, unless the label set maps the label to a nested keyword (see
  [Nested keywords](#nested-keywords)). Virtual copies of a photo get the same keywords. The grid marks
  them: "+1 virtual copy" under a photo means the original and one virtual
  copy are in the batch and both get its keywords; "virtual copy" means only
  a virtual copy was selected. The totals count each photo, so a keyword on a
  photo and its copy counts twice. It's one step
  in **Edit > Undo**. If no photo would get a keyword, it says so and adds
  nothing. If Lightroom's catalog is busy, nothing is added and the review
  window opens again with your changes, so you can click **Save All** again.
- **Cancel** (or Esc), or closing the window, adds nothing more: photos saved
  with Save Keywords keep their keywords. If you changed any photo you didn't
  save, it asks before throwing the changes away.
- If Lightroom Classic has quit while the window is open, Save Keywords and
  Save All say so instead of saving (nothing would receive the keywords), and
  Close (or Enter or Esc) closes the window. It notices within about 10 seconds of
  Lightroom quitting. Reopen Lightroom and run Classify Photos again.
- If no photo could be classified, a report opens instead of the review
  window: how many failed or were unsupported or skipped, what it ran on, the
  label set, and each photo's error. The errors can be selected and copied
  (Ctrl+C, or Cmd+C on macOS). OK, Enter or Esc closes it. It also opens where you last left it.
- The **ⓘ** button (About) shows the version, copyright and credits, and the Slint attribution its license asks for.
- While Classify Photos is running or its review is open, starting it again
  says so instead of opening a second one.

### Scores and suggestions

The app's output for each photo (in `Klassifier.log`; see
[Running the app directly](#running-the-app-directly)) has its top 10 labels
with two numbers each, and a match level:

- The **raw score** is SigLIP's own estimate that a label applies, judged on
  its own. It's low even for clear matches (often 5-30%), but it means the same
  thing in every photo, so the keyword cutoff uses it.
- The **share** is each label's part of the photo's total score across all
  labels. Shares always add up to 100%, so some label gets a big share even
  when nothing fits.
- The **match level** shows how sure the model is about the top label, from
  its raw score:
  - **strong:** raw score of 10% or more
  - **good:** 1% to 10%
  - **weak:** under 1%, so treat the labels as guesses
- **Suggested** keywords are the top labels with a raw score of at least 1%,
  at most 5 per photo. A weak match suggests none.
- A photo whose image is under 224 px on its long edge, the size the model
  works at, is flagged `small`: its labels are less reliable.

### Labels, label sets and hints

The model can only choose among the labels it's given. If a photo gets an odd
label (backstage makeup coming out as "halloween"), the list probably has
nothing closer. The fix is to add a label that describes it.

- **Label sets** are `.txt` files, one label per line, with `#` comments.
  Each label is also the name of the keyword it becomes, so the sets spell
  labels the way keywords should look (Title Case: `Bird in Flight`,
  `QR Code`). Case and punctuation don't matter to the model, which
  lowercases every label and drops punctuation (`Close-Up` and `closeup` are
  the same label to it), so changing them never changes the scores.
- **Your own label sets** go in the `Klassifier Label Sets` folder in
  Lightroom's app data folder (`~/Library/Application Support/Adobe/Lightroom/`
  on macOS, `%APPDATA%\Adobe\Lightroom\` on Windows). The folder button next to
  the **Label set** menu opens it, and creates it the first time. Add or edit
  files there at any time, even with the dialog open: the menu lists the
  folder again each time it opens. Plugin updates leave this folder alone.
  A set of yours with the same name as a built-in one replaces it, so to
  change a built-in set, copy it from `Klassifier.lrplugin/label-sets/` into
  your folder and edit the copy. Your copy then won't get the plugin's later
  changes to that set.
- **The built-in sets** are in `Klassifier.lrplugin/label-sets/`, which is
  replaced with each plugin update. To ship a new built-in set, add a file
  there and run `make deploy`. The plugin comes with:
  - **General** (the default): a bit of everything.
  - **Nature**: birds and other wildlife, sea life, insects, flowers,
    landscapes (volcanoes, lava, coast, waterfalls) and weather.
  - **City and Street**: skylines, architecture, churches, streets,
    markets, people on the street and transport.
  - **Family**: kids and family members, hugs and other moments,
    birthdays, holidays and other celebrations, school events, outings and
    pets.
  - **Fashion**: runway shows, backstage hair and makeup, and clothing.
  - **Travel**: landmarks, hotels and restaurants, planes, ships and
    trains, beaches and tours.
  - **Wedding**: the wedding party, getting ready, ceremony, reception
    moments (first dance, cutting the cake, bouquet toss) and details.
  - **Events and Performances**: concerts, theater and musicals, circus
    acts, galas and award ceremonies, parties and festivals.
  - **Sports**: most team and individual sports (including roller derby),
    game moments, venues and fans.
  - **Pets**: dogs, cats and other pets, alone or with people.
  - **Food and Drink**: meals, dishes, sweets, drinks, cooking and baking.
  - **Portraits**: the kind of portrait, its style (posed, candid, studio,
    lighting), framing and expression.
  - **Cleanup**: finds screenshots, documents, receipts and other
    non-photos, plus duds (blurry, very dark or accidental shots). Ordinary
    photos get plain labels like "child" or "landscape". Sort by the top
    label rather than the match level, which is low for this set.

  A narrow set gives more specific labels ("egret" rather than "bird") but
  can only pick from its own labels. A photo that doesn't fit the set shows
  as a weak match.
- **Hints** are extra labels for one run, typed in the dialog separated by
  commas or semicolons (e.g. `makeup, hair styling, backstage`). They're
  added to the chosen set. Tick **Use only the hints** to label photos from
  your hints alone. Hints can't map to nested keywords. While a hint contains
  `=>`, or **Use only the hints** is ticked with no hints, the dialog says so
  and **Classify** stays disabled.
- The dialog remembers the last set, hints and checkbox.

Each label becomes the prompt "a photo of <label>". Concrete nouns and short
phrases ("dog", "makeup artist", "birthday party") work better than abstract
ideas, and hints have to be labels like these, not a description of the
shoot. A new combination of labels takes a second or two the first time while
the text model computes them. They're then cached in the temp folder (`%TEMP%`, or `$TMPDIR` on macOS), and repeating
the same set and hints costs nothing extra.

### Nested keywords

A line in a label set can say which Lightroom keyword its label becomes:

```
Bird => Wildlife > Bird
Egret => Wildlife > Bird > Egret
Sunset
```

Accepting `Egret` then adds the keyword `Egret`, under `Bird` under
`Wildlife`, creating any level that doesn't exist yet (an existing one is
reused, whatever its case). The photo gets only `Egret`; Lightroom shows the
parents in the Keyword List. A line without `=>` makes a top-level keyword
named as the label.

Only the part before `=>` is shown to the model, so adding mappings doesn't
change the scores, and the cached label embeddings are still used. If the
same label appears twice with different mappings, the first one is used and
the app prints a warning (in `Klassifier.log`). A mapping with an empty
name, like `bird =>` or `bird => Wildlife >> Bird`, is an error that names
the file and line. The **Nature** set maps its birds and animals this way, as
an example. Hints can't have mappings.

Labels the model sees as the same (`Makeup` in a set and `makeup` as a hint)
are kept once, in the spelling and with the mapping seen first.

### How RAW files are read

For RAW files the app uses the JPEG preview embedded in the file and never
decodes the raw data. That preview shows the in-camera look, not your
Lightroom edits. JPEG, PNG, TIFF and WebP files are read directly. Other
formats (HEIC, PSD, ...) are reported as unsupported. Images are rotated
upright using their EXIF orientation.

### Running the app directly

```bash
klassifier [options] [paths...]
```

- `--list <file>` reads photo paths from a UTF-8 file, one per line.
- `--top <n>` reports the `n` best labels per photo (default 5).
- `--min-score <x>` suggests labels with a raw score of at least `x`, from 0
  to 1 (default 0.01). At most 5 are suggested, and only from the `--top`
  labels.
- `--labels <file>` scores against the labels in `<file>`. Repeat it to
  merge several files; a label in more than one is kept once. The default is
  `label-sets/General.txt` next to the exe.
- `--cpu` doesn't use the GPU.
- `--no-classify` only reads the previews. It needs no model files.
- `--dump <dir>` saves each loaded image, at most 1024 px, as
  `<dir>/<file name>.jpg` so you can check it by eye.
- `--thumbs <dir>` saves a thumbnail of each loaded image as `<dir>/<n>.jpg`,
  where `<n>` is the photo's position in the list plus `--thumb-start <n>`
  (default 1). `--thumb-edge <n>` sets its long edge (default 320 px). The
  plugin uses these as the review window's stand-in thumbnails.

The output is tab-separated, one line per photo (format documented in
`klassifier/src/output.rs`):

```
#klassifier	2.9.16	backend=DirectML	gpu=NVIDIA GeForce RTX 4090
#model	load_ms=520	labels=98	label_cache=hit
OK	D:\Photos\DSC09840.dng	preview	683	1024	270	22		Forest=0.1589;Tree=0.1422;Landscape=0.0235;River=0.0147;Lake=0.0074	Forest=0.4512;Tree=0.3628;Landscape=0.0681;River=0.0418;Lake=0.0306	strong	Forest;Tree;Landscape;River	
ERR	D:\Photos\IMG_0001.HEIC	unsupported: .heic
#summary	ok=1	err=1	ms=40
```

The review window is `klassifier-ui --review --in <file>`; the file it
reads and what it prints are described in `klassifier/src/ui_model.rs`.
`klassifier-ui --demo <folder>` is what `make ui-demo` runs.

## Updating the plugin

Run `make deploy`, then in **File > Plug-in Manager** select the plugin and
click **Reload Plug-in** (or restart Lightroom).

## Logs

The plugin writes to `Klassifier.log` in Lightroom's logs folder, including
the exact command line used to run `klassifier`. On macOS that's
`~/Library/Logs/Adobe/Lightroom/LrClassicLogs/Klassifier.log`.

## Uninstalling

On Windows, open **File > Plug-in Manager**, select "Klassifier", and click
**Remove**. You can then delete `C:\Users\<you>\lr-plugins\Klassifier.lrplugin`.

On macOS, quit Lightroom and delete
`~/Library/Application Support/Adobe/Lightroom/Modules/Klassifier.lrplugin`
(the Plug-in Manager can only disable plug-ins in that folder).

## Testing

```bash
make test
```

This first checks the plugin's Lua syntax with Lua 5.1's `luac` (`make lint`, also
run by `make deploy`). Lightroom runs Lua 5.1, so use that version: newer ones accept
syntax Lightroom rejects. In WSL, install it with `sudo apt install lua5.1`;
without it, the check is skipped with a note. On a Mac, where Homebrew no longer
has Lua 5.1, `make lint` builds `luac` 5.1.5 from lua.org's source into
`vendor/.cache` the first time. It catches syntax errors only, not misspelled
names or wrong SDK calls.

Next it checks the Rust formatting (`cargo fmt --check`; run `cargo fmt` in
`klassifier/` to fix it) and runs clippy with warnings as errors. Either one
failing fails `make test`. Then it runs the app's tests natively. They cover
reading previews and the review window's data (what it reads from and returns
to the plugin, and the texts it shows), not the classifier, which needs ONNX
Runtime and the model, nor the window itself.

- In WSL, the tests and this clippy pass are the Linux build, without the
  review window (so no extra Linux libraries are needed) and without the
  Windows-only code. `make clippy` (run by `make deploy`) checks those.
- On a Mac, they are the macOS build, the review window and the macOS-only
  code included.

Each platform's own code is only checked on that platform, so check a change to
shared code with `make test` on both. The tests that use real photos
read `test-data/manifest.tsv` at the repo root and are skipped if it's
missing. `test-data/` is gitignored because it holds personal photos. Each
manifest line is tab-separated:

```
<file name><TAB><source><TAB><min long edge><TAB>portrait|landscape
```

where `source` is the value the app reports (`file`, `preview`, `full` or
`thumbnail`). Lines starting with `#` are comments.

## The app icon

The exes' icon (Explorer, the taskbar), the macOS app's icon (the Dock) and
the windows' icon come from `klassifier/assets/klassifier.svg`. After editing
it, run `python3 tools/make-icons.py` (needs `pip install resvg-py pillow`) to
remake `klassifier.ico`, `klassifier.icns` and `klassifier.png` next to it, and
commit all four. The build uses those files, so building doesn't need Python.

## License

Klassifier is under the MIT license (see [LICENSE](LICENSE)). The SigLIP model,
ONNX Runtime, DirectML (Windows), the rawler library and Slint are under their own licenses; see
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md). `make deploy` copies these
files into the plugin folder.
