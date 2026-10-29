PROJECT      := SamplingReverb.xcodeproj
CONFIG       ?= Release
DERIVED      := build/DerivedData
APP          := $(DERIVED)/Build/Products/$(CONFIG)/Sampling Reverb.app
INSTALL_DIR  ?= /Applications


.PHONY: all project rust app install validate run test test-rust test-swift clean

all: app

project:
	xcodegen generate --quiet

rust:
	tools/build-rust.sh

app: project rust
	xcodebuild -project $(PROJECT) -scheme SamplingReverb -configuration $(CONFIG) \
		-derivedDataPath $(DERIVED) -quiet build

install: app
	mkdir -p "$(INSTALL_DIR)"
	rm -rf "$(INSTALL_DIR)/Sampling Reverb.app"
	cp -R "$(APP)" "$(INSTALL_DIR)/"
	pluginkit -a "$(INSTALL_DIR)/Sampling Reverb.app/Contents/PlugIns/SamplingReverbExtension.appex"
	@for i in 1 2 3 4 5 6 7 8 9 10; do auval -v aufx Smpv Crmc >/dev/null 2>&1 && break; sleep 1; done
	open "$(INSTALL_DIR)/Sampling Reverb.app"

validate:
	auval -v aufx Smpv Crmc

run: app
	open "$(APP)"

test: test-rust test-swift

# The Rust DSP core's tests.
test-rust:
	PATH="$$HOME/.cargo/bin:$$PATH" cargo test --quiet --release --manifest-path dsp/Cargo.toml --target $(shell uname -m | sed s/arm64/aarch64/)-apple-darwin

test-swift: project
	xcodebuild -project $(PROJECT) -scheme SamplingReverb -derivedDataPath $(DERIVED) -quiet test

clean:
	rm -rf build $(PROJECT)
