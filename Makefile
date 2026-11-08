# Builds and deploys the plugin for the host's platform: macOS (Apple Silicon) on a Mac,
# Windows (cross-compiled) in WSL. `make dist` assembles the plugin in dist/ (gitignored);
# `make deploy` copies it to where Lightroom loads it. What differs per platform (Rust target,
# ONNX Runtime files, the apps, the deploy folder, reveal, ui-demo) is in mk/$(PLATFORM).mk;
# the rest is here.
PLATFORM    ?= $(if $(filter Darwin,$(shell uname -s)),macos,windows)
PLUGIN      := Klassifier.lrplugin
DIST        := dist/$(PLUGIN)
RELEASE      = klassifier/target/$(TARGET)/release

# Runtime files that aren't committed: ONNX Runtime ($(RUNTIME_DIR), per platform) and the
# SigLIP model (shared). `make fetch` downloads them; `make dist` puts them next to the apps.
VENDOR      := vendor
CACHE       := $(VENDOR)/.cache
RUNTIME_DIR := $(VENDOR)/$(PLATFORM)
MODEL_DIR   := $(VENDOR)/models/siglip
# ONNX Runtime must provide the C API version selected by the `ort` crate's api-NN feature.
ORT_VERSION := 1.24.4
SIGLIP_URL  := https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/4649052661e53c7000355844105f8a1792088239
# SHA-256 of each download, pinned from the files fetched on 2026-11-04.
VISION_SHA256    := f89d41bac7f4d4b87e010a467d93f98689d708916ed22f5a07f96fdfa26f475f
TEXT_SHA256      := 3aa7fdbd20eaa8740cce17bf82913de641fcb632a768fed59f661cdcd0c32553
TOKENIZER_SHA256 := 4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08
MODELS      := $(MODEL_DIR)/vision_model.onnx $(MODEL_DIR)/text_model.onnx $(MODEL_DIR)/tokenizer.json

# $(call download,<url>,<sha256>): fetch to a .part file, check its SHA-256, then keep it, so
# an interrupted or altered download is never used.
download = mkdir -p $(@D) && curl -fL --retry 3 -o $@.part $(1) && \
	{ echo "$(2)  $@.part" | sha256sum -c --quiet - || { rm -f $@.part; echo "SHA-256 mismatch: $(1)" >&2; exit 1; }; } && \
	mv $@.part $@
# $(call extract,<zip>,<member>): copy one file out of a zip (a .nupkg is one) to $@.
extract = mkdir -p $(@D) && python3 -I -c 'import sys, zipfile; open(sys.argv[3], "wb").write(zipfile.ZipFile(sys.argv[1]).read(sys.argv[2]))' $(1) $(2) $@
# $(call extract-tgz,<tgz>,<member>): the same for a .tgz.
extract-tgz = mkdir -p $(@D) && python3 -I -c 'import sys, tarfile; open(sys.argv[3], "wb").write(tarfile.open(sys.argv[1]).extractfile(sys.argv[2]).read())' $(1) $(2) $@

.PHONY: all bump build cargo-build lint test clippy fetch dist check-version deploy reveal ui-demo clean

all: deploy

# Sets TARGET, RUNTIME, DIST_FILES (the apps), DEPLOY_DIR, TEST_FLAGS, LUAC, the reveal and
# ui-demo rules, and optionally PACKAGE (build steps after cargo) and LUAC_DEP (a rule that
# builds $(LUAC)).
include mk/$(PLATFORM).mk

cargo-build:
	cd klassifier && cargo build --release --target $(TARGET)

build: cargo-build $(PACKAGE)

# Syntax check only (-p writes no luac.out). Lightroom runs Lua 5.1; a newer luac accepts
# syntax Lightroom rejects (goto, //, bitwise ops). Skipped with a note if there's no luac 5.1.
lint: $(LUAC_DEP)
	@if command -v $(LUAC) >/dev/null; then \
		$(LUAC) -p $(PLUGIN)/*.lua && echo "Lua syntax OK"; \
	else \
		echo "Skipping Lua syntax check: $(LUAC) not found ($(LUAC_HINT))"; \
	fi

test: lint
	cd klassifier && cargo fmt --check
	cd klassifier && cargo clippy $(TEST_FLAGS) --all-targets -- -D warnings
	cd klassifier && cargo test $(TEST_FLAGS)

# The deployed build, the review window and platform-specific code included.
clippy:
	cd klassifier && cargo clippy --release --target $(TARGET) --all-targets -- -D warnings

fetch: $(RUNTIME) $(MODELS)

$(MODEL_DIR)/vision_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F),$(VISION_SHA256))

$(MODEL_DIR)/text_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F),$(TEXT_SHA256))

$(MODEL_DIR)/tokenizer.json:
	$(call download,$(SIGLIP_URL)/$(@F),$(TOKENIZER_SHA256))

# `make bump` (bug fixes, docs, tooling), `make bump PART=minor` (features) or PART=major:
# the next version in Info.lua and Cargo.toml, build + 1.
PART ?= revision
bump:
	tools/bump-version.sh $(PART)

# Every build gets a new version (CLAUDE.md): deploying the last deployed version, or one
# where Info.lua and Cargo.toml disagree, fails here. The version is recorded once the copy
# succeeds, so a failed deploy can be retried. Each deploy folder keeps its own record.
check-version:
	tools/check-version.sh $(PLUGIN) "$(DEPLOY_DIR)/$(PLUGIN)"

# The finished plugin folder: the plugin's Lua, the apps, ONNX Runtime, the model and the
# licenses. The model files are hard links to vendor/'s, so dist/ adds no 800 MB copy.
dist: build fetch
	mkdir -p $(DIST)/models/siglip
	rsync -rt --delete --exclude=/models "$(PLUGIN)/" "$(RUNTIME_DIR)/" \
		$(DIST_FILES) LICENSE THIRD_PARTY_NOTICES.md licenses $(DIST)/
	ln -f $(MODELS) $(DIST)/models/siglip/
	@echo "Built $(DIST)"

# A copy of dist/, so what Lightroom runs changes only here, after the checks. rsync only
# copies changed files, so the large model files are copied once.
deploy: check-version lint clippy dist
	mkdir -p "$(DEPLOY_DIR)/$(PLUGIN)"
	rsync -rt --delete --exclude=.deployed-version "$(DIST)/" "$(DEPLOY_DIR)/$(PLUGIN)/"
	tools/check-version.sh --stamp $(PLUGIN) "$(DEPLOY_DIR)/$(PLUGIN)"
	@echo "Deployed to $(DEPLOY_DIR)/$(PLUGIN)"

# Opens the review window with made-up labels, to try UI changes without Lightroom. Stand-in
# thumbnails are made from the photos in $(DEMO_PHOTOS); the recipe is in mk/$(PLATFORM).mk.
# `make ui-demo DEMO_ARGS="--screenshot shot.png"` saves a picture of the window instead.
DEMO_PHOTOS ?= test-data
DEMO_COUNT  ?= 500
DEMO_DIR    := $(CACHE)/ui-demo

clean:
	cd klassifier && cargo clean
	rm -rf dist
