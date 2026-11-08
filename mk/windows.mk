# Windows: cross-compiled in WSL with MinGW, running on DirectML. Included by the Makefile.
TARGET       := x86_64-pc-windows-gnu
LINUX        := x86_64-unknown-linux-gnu
DEPLOY_DIR   ?= /mnt/c/Users/cadre/lr-plugins
DEPLOY_FILES  = $(RELEASE)/klassifier.exe $(RELEASE)/klassifier-ui.exe
LUAC         ?= luac5.1
LUAC_HINT    := sudo apt install lua5.1

# The tests run natively on Linux, without the `ui` feature: Slint needs system libraries to
# build on Linux, and the review window's logic (src/ui_model.rs) is in the lib, which is
# tested either way. `make clippy` checks the Windows build, the review window included.
TEST_FLAGS   := --target $(LINUX) --no-default-features

# ONNX Runtime with DirectML, and DirectML itself, from NuGet.
NUGET        := https://api.nuget.org/v3-flatcontainer
DML_VERSION  := 1.15.4
ORT_SHA256   := 57e9f11b73437bef7a309496135d4c1f96b1a8e9ddba60013fa27bfc1d788681
DML_SHA256   := 4e7cb7ddce8cf837a7a75dc029209b520ca0101470fcdf275c1f49736a3615b9
ORT_PKG      := $(CACHE)/onnxruntime-directml-$(ORT_VERSION).nupkg
DML_PKG      := $(CACHE)/directml-$(DML_VERSION).nupkg
RUNTIME      := $(RUNTIME_DIR)/onnxruntime.dll $(RUNTIME_DIR)/onnxruntime_providers_shared.dll \
                $(RUNTIME_DIR)/DirectML.dll

$(ORT_PKG):
	$(call download,$(NUGET)/microsoft.ml.onnxruntime.directml/$(ORT_VERSION)/microsoft.ml.onnxruntime.directml.$(ORT_VERSION).nupkg,$(ORT_SHA256))

$(DML_PKG):
	$(call download,$(NUGET)/microsoft.ai.directml/$(DML_VERSION)/microsoft.ai.directml.$(DML_VERSION).nupkg,$(DML_SHA256))

$(RUNTIME_DIR)/onnxruntime.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime.dll)

$(RUNTIME_DIR)/onnxruntime_providers_shared.dll: $(ORT_PKG)
	$(call extract,$<,runtimes/win-x64/native/onnxruntime_providers_shared.dll)

$(RUNTIME_DIR)/DirectML.dll: $(DML_PKG)
	$(call extract,$<,bin/x64-win/DirectML.dll)

# Opens the review window on Windows. The stand-in thumbnails are made by a Linux build of
# the app (it can't run the .exe).
ui-demo: build
	mkdir -p $(DEMO_DIR)
	cd klassifier && cargo run --release --quiet --target $(LINUX) --no-default-features --bin klassifier -- \
		--no-classify --thumbs $(abspath $(DEMO_DIR)) $(abspath $(DEMO_PHOTOS))/* > /dev/null
	$(RELEASE)/klassifier-ui.exe --demo "$$(wslpath -w $(DEMO_DIR))" --count $(DEMO_COUNT) $(DEMO_ARGS)
