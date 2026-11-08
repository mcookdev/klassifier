# macOS on Apple Silicon: built natively, running on WebGPU (Metal). Included by the Makefile.
# Lightroom Classic loads every plug-in in DEPLOY_DIR at startup, no Plug-in Manager needed.
TARGET       := aarch64-apple-darwin
DEPLOY_DIR   ?= $(HOME)/Library/Application Support/Adobe/Lightroom/Modules
TEST_FLAGS   := --target $(TARGET)

# The review window is an app bundle, so it has the Klassifier name and icon in the Dock and
# Cmd-Tab. The plugin runs its binary directly (Exec.lua). klassifier is a plain binary.
APP          := $(RELEASE)/Klassifier.app
DEPLOY_FILES  = $(RELEASE)/klassifier $(APP)
PACKAGE      := app
# The bundle's version: Cargo.toml's, and Info.lua's build number.
VERSION       = $(shell sed -n 's/^version *= *"\(.*\)"/\1/p' klassifier/Cargo.toml | head -n 1)
BUILD_NUMBER  = $(shell sed -n 's/.*build *= *\([0-9]*\).*/\1/p' $(PLUGIN)/Info.lua)

# ONNX Runtime's macOS build (WebGPU included), from its GitHub release.
ORT_SHA256   := 93787795f47e1eee369182e43ed51b9e5da0878ab0346aecf4258979b8bba989
ORT_PKG      := $(CACHE)/onnxruntime-osx-arm64-$(ORT_VERSION).tgz
RUNTIME      := $(RUNTIME_DIR)/libonnxruntime.dylib $(RUNTIME_DIR)/onnxruntime-ThirdPartyNotices.txt

# Homebrew no longer has Lua 5.1, so `make lint` builds luac 5.1 from lua.org's source.
LUA_VERSION  := 5.1.5
LUA_SHA256   := 2640fc56a795f29d28ef15e13c34a47e223960b0240e8cb0a82d9b0738695333
LUA_PKG      := $(CACHE)/lua-$(LUA_VERSION).tar.gz
LUA51        := $(CACHE)/lua-$(LUA_VERSION)/src/luac
LUAC         ?= $(LUA51)
LUAC_DEP     := $(filter $(LUA51),$(LUAC))
LUAC_HINT    := run make lint without LUAC set to build it

$(ORT_PKG):
	$(call download,https://github.com/microsoft/onnxruntime/releases/download/v$(ORT_VERSION)/$(@F),$(ORT_SHA256))

# Saved without the version in its name: the app loads libonnxruntime.dylib by path.
$(RUNTIME_DIR)/libonnxruntime.dylib: $(ORT_PKG)
	$(call extract-tgz,$<,onnxruntime-osx-arm64-$(ORT_VERSION)/lib/libonnxruntime.$(ORT_VERSION).dylib)

# The licenses of what that build contains (Dawn, for WebGPU, among them), deployed with it.
$(RUNTIME_DIR)/onnxruntime-ThirdPartyNotices.txt: $(ORT_PKG)
	$(call extract-tgz,$<,onnxruntime-osx-arm64-$(ORT_VERSION)/ThirdPartyNotices.txt)

$(LUA_PKG):
	$(call download,https://www.lua.org/ftp/$(@F),$(LUA_SHA256))

$(LUA51): $(LUA_PKG)
	tar -xzf $< -C $(CACHE)
	$(MAKE) -C $(@D) luac MYCFLAGS=-DLUA_USE_POSIX CC="cc -w"

# The bundle, rebuilt on each build (it's small). Ad-hoc signed as a whole, so macOS sees one
# consistent app; nothing is notarized, which a locally built app doesn't need.
.PHONY: app
app: cargo-build
	rm -rf $(APP)
	mkdir -p $(APP)/Contents/MacOS $(APP)/Contents/Resources
	sed -e 's/@VERSION@/$(VERSION)/g' -e 's/@BUILD@/$(BUILD_NUMBER)/g' klassifier/assets/macos/Info.plist > $(APP)/Contents/Info.plist
	cp $(RELEASE)/klassifier-ui $(APP)/Contents/MacOS/
	cp klassifier/assets/klassifier.icns $(APP)/Contents/Resources/
	codesign --force --sign - $(APP)

# Opens the review window. The stand-in thumbnails are made by the same native build of the app.
ui-demo: build
	mkdir -p $(DEMO_DIR)
	cd klassifier && cargo run --release --quiet --target $(TARGET) --bin klassifier -- \
		--no-classify --thumbs $(abspath $(DEMO_DIR)) $(abspath $(DEMO_PHOTOS))/* > /dev/null
	$(APP)/Contents/MacOS/klassifier-ui --demo $(DEMO_DIR) --count $(DEMO_COUNT) $(DEMO_ARGS)
