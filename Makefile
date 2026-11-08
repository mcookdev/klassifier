PLUGIN      := Klassifier.lrplugin
EXE         := klassifier/target/x86_64-pc-windows-gnu/release/klassifier.exe
EXE_UI      := klassifier/target/x86_64-pc-windows-gnu/release/klassifier-ui.exe
LINUX       := x86_64-unknown-linux-gnu
DEPLOY_DIR  ?= /mnt/c/Users/cadre/lr-plugins
# Lightroom runs Lua 5.1; a newer luac accepts syntax Lightroom rejects (goto, //, bitwise ops).
LUAC        ?= luac5.1

# Runtime files that aren't committed: ONNX Runtime + DirectML DLLs and the SigLIP model.
# `make fetch` downloads them into $(VENDOR); `make deploy` copies them next to the exe.
VENDOR      := vendor
CACHE       := $(VENDOR)/.cache
MODEL_DIR   := $(VENDOR)/models/siglip
NUGET       := https://api.nuget.org/v3-flatcontainer
# ONNX Runtime must provide the C API version selected by the `ort` crate's api-NN feature.
ORT_VERSION := 1.24.4
DML_VERSION := 1.15.4
SIGLIP_URL  := https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/4649052661e53c7000355844105f8a1792088239
# SHA-256 of each download, pinned from the files fetched on 2026-11-04.
ORT_SHA256       := 57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681
DML_SHA256       := 4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9
VISION_SHA256    := f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f
TEXT_SHA256      := 3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553
TOKENIZER_SHA256 := 4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08

ORT_PKG     := $(CACHE)/onnxruntime-directml-$(ORT_VERSION).nupkg
DML_PKG     := $(CACHE)/directml-$(DML_VERSION).nupkg
ASSETS      := $(VENDOR)/onnxruntime.dll $(VENDOR)/onnxruntime_providers_shared.dll \
               $(VENDOR)/DirectML.dll \
               $(MODEL_DIR)/vision_model.onnx $(MODEL_DIR)/text_model.onnx $(MODEL_DIR)/tokenizer.json

# $(call download,<url>,<sha256>): fetch to a .part file, check its SHA-256, then keep it, so
# an interrupted or altered download is never used.
download = mkdir -p $(@D) && curl -fL --retry 3 -o $@.part $(1) && \
	{ echo "$(2)  $@.part" | sha256sum -c --quiet - || { rm -f $@.part; echo "SHA-256 mismatch: $(1)" >&2; exit 1; }; } && \
	mv $@.part $@
# $(call extract,<zip>,<member>): copy one file out of a .nupkg (a zip) to $@.
extract = mkdir -p $(@D) && python3 -I -c 'import sys, zipfile; open(sys.argv[3], "wb").write(zipfile.ZipFile(sys.argv[1]).read(sys.argv[2]))' $(1) $(2) $@

.PHONY: all build lint test clippy-windows fetch check-version deploy ui-demo clean

all: deploy

build:
	cd klassifier && cargo build --release

# Syntax check only (-p writes no luac.out). Skipped with a note if luac5.1 isn't installed.
lint:
	@if command -v $(LUAC) >/dev/null; then \
		$(LUAC) -p $(PLUGIN)/*.lua && echo "Lua syntax OK"; \
	else \
		echo "Skipping Lua syntax check: $(LUAC) not found (sudo apt install lua5.1)"; \
	fi

# Without the `ui` feature: Slint needs system libraries to build on Linux, and the review
# window's logic (src/ui_model.rs) is in the lib, which is tested either way.
test: lint
	cd klassifier && cargo fmt --check
	cd klassifier && cargo clippy --target $(LINUX) --no-default-features --all-targets -- -D warnings
	cd klassifier && cargo test --target $(LINUX) --no-default-features

# The Windows build, the review window and cfg(windows) code included, which `make test` skips.
clippy-windows:
	cd klassifier && cargo clippy --release --all-targets -- -D warnings

fetch: $(ASSETS)

$(ORT_PKG):
	$(call download,$(NUGET)/microsoft.ml.onnxruntime.directml/$(ORT_VERSION)/microsoft.ml.onnxruntime.directml.$(ORT_VERSION).nupkg,$(ORT_SHA256))

$(DML_PKG):
	$(call download,$(NUGET)/microsoft.ai.directml/$(DML_VERSION)/microsoft.ai.directml.$(DML_VERSION).nupkg,$(DML_SHA256))

$(VENDOR)/onnxruntime.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime.dll)

$(VENDOR)/onnxruntime_providers_shared.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime_providers_shared.dll)

$(VENDOR)/DirectML.dll: $(DML_PKG)
	$(call extract,$<,bin/x64-win/DirectML.dll)

$(MODEL_DIR)/vision_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F),$(VISION_SHA256))

$(MODEL_DIR)/text_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F),$(TEXT_SHA256))

$(MODEL_DIR)/tokenizer.json:
	$(call download,$(SIGLIP_URL)/$(@F),$(TOKENIZER_SHA256))

# Every build gets a new version (CLAUDE.md): deploying the last deployed version, or one
# where Info.lua and Cargo.toml disagree, fails here. The version is recorded once the copy
# succeeds, so a failed deploy can be retried.
check-version:
	tools/check-version.sh $(PLUGIN) "$(DEPLOY_DIR)/$(PLUGIN)"

# rsync only copies changed files, so the large model files are copied once.
deploy: check-version lint clippy-windows build fetch
	mkdir -p "$(DEPLOY_DIR)/$(PLUGIN)"
	rsync -rt --delete --exclude=.cache --exclude=.deployed-version "$(PLUGIN)/" "$(VENDOR)/" "$(EXE)" "$(EXE_UI)" \
		LICENSE THIRD_PARTY_NOTICES.md licenses "$(DEPLOY_DIR)/$(PLUGIN)/"
	tools/check-version.sh --stamp $(PLUGIN) "$(DEPLOY_DIR)/$(PLUGIN)"
	@echo "Deployed to $(DEPLOY_DIR)/$(PLUGIN)"

# Opens the review window on Windows with made-up labels, to try UI changes without Lightroom.
# Stand-in thumbnails are made from the photos in $(DEMO_PHOTOS) by a Linux build of the app.
# `make ui-demo DEMO_ARGS="--screenshot shot.png"` saves a picture of the window instead.
DEMO_PHOTOS ?= test-data
DEMO_COUNT  ?= 500
DEMO_DIR    := $(CACHE)/ui-demo
ui-demo: build
	mkdir -p $(DEMO_DIR)
	cd klassifier && cargo run --release --quiet --target $(LINUX) --no-default-features --bin klassifier -- \
		--no-classify --thumbs ../$(DEMO_DIR) ../$(DEMO_PHOTOS)/* > /dev/null
	$(EXE_UI) --demo "$$(wslpath -w $(DEMO_DIR))" --count $(DEMO_COUNT) $(DEMO_ARGS)

clean:
	cd klassifier && cargo clean
