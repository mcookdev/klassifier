PLUGIN      := Klassifier.lrplugin
EXE         := klassifier/target/x86_64-pc-windows-gnu/release/klassifier.exe
DEPLOY_DIR  ?= /mnt/c/Users/cadre/lr-plugins
# Lightroom runs Lua 5.1; a newer luac accepts syntax Lightroom rejects (goto, //, bitwise ops).
LUAC        ?= luac5.1

# Runtime files that aren't committed: ONNX Runtime + DirectML DLLs and the SigLIP models.
# `make fetch` downloads them into $(VENDOR); `make deploy` copies them next to the exe.
VENDOR      := vendor
CACHE       := $(VENDOR)/.cache
# One folder per model, named as the app's --model option (see MODELS in klassifier/src/siglip.rs).
# Each holds vision_model.onnx, text_model.onnx (plus text_model.onnx_data for the fp32 SigLIP 2,
# whose weights don't fit in one ONNX file) and tokenizer.json.
LIGHT_DIR   := $(VENDOR)/models/siglip-base-224
MEDIUM_DIR  := $(VENDOR)/models/siglip2-so400m-fp16
HIGH_DIR    := $(VENDOR)/models/siglip2-so400m-fp32
NUGET       := https://api.nuget.org/v3-flatcontainer
# ONNX Runtime must provide the C API version selected by the `ort` crate's api-NN feature.
ORT_VERSION := 1.24.4
DML_VERSION := 1.15.4
SIGLIP_URL  := https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/4649052661e53c7000355844105f8a1792088239
SIGLIP2_URL := https://huggingface.co/onnx-community/siglip2-so400m-patch16-384-ONNX/resolve/29f74e673c298fd07482d8e20138179282001eb7

ORT_PKG     := $(CACHE)/onnxruntime-directml-$(ORT_VERSION).nupkg
DML_PKG     := $(CACHE)/directml-$(DML_VERSION).nupkg
ASSETS      := $(VENDOR)/onnxruntime.dll $(VENDOR)/onnxruntime_providers_shared.dll \
               $(VENDOR)/DirectML.dll \
               $(foreach d,$(LIGHT_DIR) $(MEDIUM_DIR) $(HIGH_DIR),$(d)/vision_model.onnx $(d)/text_model.onnx $(d)/tokenizer.json) \
               $(HIGH_DIR)/text_model.onnx_data

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

$(LIGHT_DIR)/vision_model.onnx $(LIGHT_DIR)/text_model.onnx:
	$(call download,$(SIGLIP_URL)/onnx/$(@F))

$(LIGHT_DIR)/tokenizer.json:
	$(call download,$(SIGLIP_URL)/$(@F))

# The fp16 files take and return fp32 tensors, so the app treats them like the fp32 ones.
$(MEDIUM_DIR)/vision_model.onnx $(MEDIUM_DIR)/text_model.onnx:
	$(call download,$(SIGLIP2_URL)/onnx/$(basename $(@F))_fp16.onnx)

# text_model.onnx names its weights file, so that one must keep the name text_model.onnx_data.
$(HIGH_DIR)/vision_model.onnx $(HIGH_DIR)/text_model.onnx $(HIGH_DIR)/text_model.onnx_data:
	$(call download,$(SIGLIP2_URL)/onnx/$(@F))

$(MEDIUM_DIR)/tokenizer.json $(HIGH_DIR)/tokenizer.json:
	$(call download,$(SIGLIP2_URL)/$(@F))

# rsync only copies changed files, so the large model files are copied once.
deploy: lint build fetch
	mkdir -p "$(DEPLOY_DIR)/$(PLUGIN)"
	rsync -rt --delete --exclude=.cache "$(PLUGIN)/" "$(VENDOR)/" "$(EXE)" "$(DEPLOY_DIR)/$(PLUGIN)/"
	@echo "Deployed to $(DEPLOY_DIR)/$(PLUGIN)"

clean:
	cd klassifier && cargo clean
