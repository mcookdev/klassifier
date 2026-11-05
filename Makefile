PLUGIN      := Klassifier.lrplugin
EXE         := klassifier/target/x86_64-pc-windows-gnu/release/klassifier.exe
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

ORT_PKG     := $(CACHE)/onnxruntime-directml-$(ORT_VERSION).nupkg
DML_PKG     := $(CACHE)/directml-$(DML_VERSION).nupkg
ASSETS      := $(VENDOR)/onnxruntime.dll $(VENDOR)/onnxruntime_providers_shared.dll \
               $(VENDOR)/DirectML.dll \
               $(MODEL_DIR)/vision_model.onnx $(MODEL_DIR)/text_model.onnx $(MODEL_DIR)/tokenizer.json

# $(call download,<url>): fetch to a .part file first so an interrupted download isn't kept.
download = mkdir -p $(@D) && curl -fL --retry 3 -o $@.part $(1) && mv $@.part $@
# $(call extract,<zip>,<member>): copy one file out of a .nupkg (a zip) to $@.
extract = mkdir -p $(@D) && python3 -I -c 'import sys, zipfile; open(sys.argv[3], "wb").write(zipfile.ZipFile(sys.argv[1]).read(sys.argv[2]))' $(1) $(2) $@

.PHONY: all build lint test fetch deploy clean

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

test: lint
	cd klassifier && cargo test --target x86_64-unknown-linux-gnu

fetch: $(ASSETS)

$(ORT_PKG):
	$(call download,$(NUGET)/microsoft.ml.onnxruntime.directml/$(ORT_VERSION)/microsoft.ml.onnxruntime.directml.$(ORT_VERSION).nupkg)

$(DML_PKG):
	$(call download,$(NUGET)/microsoft.ai.directml/$(DML_VERSION)/microsoft.ai.directml.$(DML_VERSION).nupkg)

$(VENDOR)/onnxruntime.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime.dll)

$(VENDOR)/onnxruntime_providers_shared.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime_providers_shared.dll)

$(VENDOR)/DirectML.dll: $(DML_PKG)
	$(call extract,$<,bin/x64-win/DirectML.dll)

$(MODEL_DIR)/vision_model.onnx $(MODEL_DIR)/text_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F))

$(MODEL_DIR)/tokenizer.json:
	$(call download,$(SIGLIP_URL)/$(@F))

# rsync only copies changed files, so the large model files are copied once.
deploy: lint build fetch
	mkdir -p "$(DEPLOY_DIR)/$(PLUGIN)"
	rsync -rt --delete --exclude=.cache "$(PLUGIN)/" "$(VENDOR)/" "$(EXE)" "$(DEPLOY_DIR)/$(PLUGIN)/"
	@echo "Deployed to $(DEPLOY_DIR)/$(PLUGIN)"

clean:
	cd klassifier && cargo clean
