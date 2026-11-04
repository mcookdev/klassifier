PLUGIN      := HelloWorld.lrplugin
EXE         := hello-app/target/x86_64-pc-windows-gnu/release/hello-app.exe
DEPLOY_DIR  ?= /mnt/c/Users/cadre/lr-plugins

.PHONY: all build test deploy clean

all: deploy

build:
	cd hello-app && cargo build --release

test:
	cd hello-app && cargo test --target x86_64-unknown-linux-gnu

deploy: build
	rm -rf "$(DEPLOY_DIR)/$(PLUGIN)"
	mkdir -p "$(DEPLOY_DIR)"
	cp -r "$(PLUGIN)" "$(DEPLOY_DIR)/"
	cp "$(EXE)" "$(DEPLOY_DIR)/$(PLUGIN)/"
	@echo "Deployed to $(DEPLOY_DIR)/$(PLUGIN)"

clean:
	cd hello-app && cargo clean
