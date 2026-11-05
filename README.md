# Klassifier

A Lightroom Classic plug-in that adds a "Classify Photos" command under
**File > Plug-in Extras** and **Library > Plug-in Extras**. It runs a small
Windows console app (`klassifier.exe`, written in Rust) on the targeted photos.
The app scores each photo against a list of labels ("dog", "beach",
"sunset", ...) with the [SigLIP](https://huggingface.co/google/siglip-base-patch16-224)
image model, and the plugin shows the suggested keywords for each photo in a
review window. You can accept them all at once, or adjust a photo's keywords
first, and the plugin adds them to the photos in the catalog.

The model runs on the GPU through DirectML (any DirectX 12 GPU) and falls back
to the CPU. This small model is only slightly slower on the CPU. On a PC with
more than one GPU (say a desktop CPU's integrated graphics and a graphics
card), it uses the one Windows ranks as high-performance, normally the
graphics card. The report's summary names it ("Ran on DirectML (NVIDIA GeForce
RTX 4090)").

For RAW files the app uses the JPEG preview embedded in the file and never
decodes the raw data. JPEG, PNG, TIFF and WebP files are read directly. Other
formats (HEIC, PSD, ...) are reported as unsupported. Images are rotated
upright using their EXIF orientation.

## Requirements

- Adobe Lightroom Classic (SDK 6.0 or later) on Windows
- WSL with Rust (`cargo`), `make`, `curl`, `python3` and `rsync` for building
  (the last three are usually preinstalled on Ubuntu)

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

Or press **Ctrl+Shift+B** in VS Code. This cross-compiles `klassifier.exe` and
copies the plugin folder, the exe and the runtime files to
`C:\Users\cadre\lr-plugins\Klassifier.lrplugin`. Only changed files are
copied, so the model is copied once. To deploy somewhere else:

```bash
make deploy DEPLOY_DIR=/mnt/c/somewhere
```

## Testing

```bash
make test
```

This first checks the plugin's Lua syntax with `luac5.1` (`make lint`, also run by
`make deploy`). Lightroom runs Lua 5.1, so use that version: newer ones accept
syntax Lightroom rejects. Install it with `sudo apt install lua5.1`. Without it,
the check is skipped with a note. It catches syntax errors only, not misspelled
names or wrong SDK calls.

It then runs the app's tests natively on Linux. They cover reading previews, not
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
3. Click **Add** and select `C:\Users\cadre\lr-plugins\Klassifier.lrplugin`.
   Don't add the repo's `Klassifier.lrplugin` folder; it doesn't contain the exe
   or the model.
4. Confirm the plugin shows up as "Klassifier" with status
   "Installed and running". Click **Done**.

## Usage

- Select one or more photos (optional).
- Go to **File > Plug-in Extras > Classify Photos** (or **Library > Plug-in
  Extras > Classify Photos**).
- A dialog asks for the label set and optional hints (see below), then
  classifies when you click **Classify N photos**. If nothing is selected,
  Lightroom targets the whole filmstrip, so check the count. Above 500 photos
  the dialog warns that it can take a while.
- Videos are skipped. Virtual copies that share a file with another targeted
  photo are processed only once, and share its suggestions.
- Photos go to `klassifier.exe` 100 at a time, with a progress bar. Cancel
  takes effect between batches. A console window may flash for each batch.
  Each batch spends about half a second loading the model, then roughly
  30 ms per ARW and 60-120 ms per CR2 or JPEG.
- The **Review keywords** window shows the photos 12 at a time (4x3), each
  with its file name and a button showing how many keywords it will get
  ("3 Keywords", "0 Keywords" when nothing was suggested, or
  "2 Keywords (edited)" once you've changed them). Use
  **< Prev** and **Next >** to page through them. The top shows the totals,
  the label set, and how many files failed, were unsupported or were skipped.
- That button opens the photo's keyword view: a larger preview and
  a checkbox for each of its top 10 labels, best first, with the suggested
  ones ticked. Tick or untick them and click **Accept** to keep your choice,
  or **Cancel** to leave it as it was. Nothing is written yet; the photo now
  shows as edited.
- **Accept All** (at the top or bottom) adds each photo's keywords: your
  choice for edited photos, the suggestions for the rest. Photos with no
  suggestions that you didn't edit get nothing. Keywords are only added,
  never removed, and a keyword that already exists is reused, whatever its
  case (`bird` reuses `Bird`). New keywords are top level, named as in the
  label set, unless the label set maps the label to a nested keyword (see
  [Nested keywords](#nested-keywords)); the keyword view then shows where it
  goes, e.g. `Egret   (Wildlife > Bird > Egret)`. Virtual copies of a photo get the same keywords. The grid marks
  them: "+1 virtual copy" under a photo means the original and one virtual
  copy are in the batch and both get its keywords; "virtual copy" means only
  a virtual copy was selected. The totals count each photo, so a keyword on a
  photo and its copy counts twice. It's one step
  in **Edit > Undo**. If no photo would get a keyword, it says so and adds
  nothing. If Lightroom's catalog is busy, nothing is added and the review
  window opens again with your choices kept, so you can click **Accept All**
  again.
- **Cancel** closes the window without changing anything.
- **Details...** opens the classification report: each photo's match level,
  its top 10 labels and suggestions, the preview size and where it came from,
  and errors. If no photo could be classified, the report opens instead of
  the review window.

### Reading the report (Details...)

```
IMG_7925.CR2  -  strong match    (3456x5184 preview, 116 ms)
        Halloween 93%, Costume 4%, Child 1%, Baby <1%, Holiday <1%, Party <1%, Mask <1%, Makeup <1%, Portrait <1%, Family <1%
        Suggested: Halloween, Costume
```

- **SMALL PREVIEW** after the heading means the image the app read is
  under 224 px on its long edge, the size the model works at, so the labels
  are less reliable. The summary at the top counts these.
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
and is what the keyword cutoff uses, since it means the same thing in every
photo.

- **Suggested** keywords are the top labels with a raw score of at least 1%,
  at most 5 per photo. A weak match suggests none.

### Labels, label sets and hints

The model can only choose among the labels it's given. If a photo gets an odd
label (backstage makeup coming out as "halloween"), the list probably has
nothing closer. The fix is to add a label that describes it.

- **Label sets** are the `.txt` files in `Klassifier.lrplugin/label-sets/`,
  one label per line, with `#` comments. Each label is also the name of the
  keyword it becomes, so the sets spell labels the way keywords should look
  (Title Case: `Bird in Flight`, `QR Code`). Case and punctuation don't
  matter to the model, which lowercases every label and drops punctuation
  (`Close-Up` and `closeup` are the same label to it), so changing them
  never changes the scores. To add a set, add a file and run
  `make deploy`. It then appears in the dialog's **Label set** menu. The
  plugin comes with:
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
  your hints alone. Hints can't map to nested keywords: the dialog refuses a
  hint containing `=>`.
- The dialog remembers the last set, hints and checkbox.

Each label becomes the prompt "a photo of <label>". Concrete nouns and short
phrases ("dog", "makeup artist", "birthday party") work better than abstract
ideas, and hints have to be labels like these, not a description of the
shoot. A new combination of labels takes a second or two the first time while
the text model computes them. They're then cached in `%TEMP%`, and repeating
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
klassifier.exe [options] [paths...]
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

The output is tab-separated, one line per photo (format documented in
`klassifier/src/output.rs`):

```
#klassifier	2.4.0	backend=DirectML	gpu=NVIDIA GeForce RTX 4090
#model	load_ms=520	labels=98	label_cache=hit
OK	D:\Photos\DSC09840.dng	preview	683	1024	270	22		Forest=0.1589;Tree=0.1422;Landscape=0.0235;River=0.0147;Lake=0.0074	Forest=0.4512;Tree=0.3628;Landscape=0.0681;River=0.0418;Lake=0.0306	strong	Forest;Tree;Landscape;River	
ERR	D:\Photos\IMG_0001.HEIC	unsupported: .heic
#summary	ok=1	err=1	ms=40
```

## Updating the plugin

Run `make deploy`, then in **File > Plug-in Manager** select the plugin and
click **Reload Plug-in**.

## Logs

The plugin writes to `Klassifier.log` in Lightroom's logs folder, including
the exact command line used to run `klassifier.exe`. Open it from the
Plug-in Manager's log file link.

## Uninstalling

Open **File > Plug-in Manager**, select "Klassifier", and click
**Remove**. You can then delete `C:\Users\cadre\lr-plugins\Klassifier.lrplugin`.
