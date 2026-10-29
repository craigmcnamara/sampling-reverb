PROJECT      := SamplingReverb.xcodeproj
CONFIG       ?= Release
DERIVED      := build/DerivedData
APP          := $(DERIVED)/Build/Products/$(CONFIG)/Sampling Reverb.app
INSTALL_DIR  ?= /Applications

CXX          ?= clang++
DSP_FLAGS    := -std=c++17 -O2 -Wall -Wextra -IExtension/DSP -ITests/DSP
DSP_TEST_BIN := build/dsp_tests

.PHONY: all project rust app install validate run test test-dsp test-rust test-parity test-swift clean

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

test: test-dsp test-rust test-parity test-swift

# The Rust DSP core's own tests (on the host architecture).
test-rust:
	PATH="$$HOME/.cargo/bin:$$PATH" cargo test --quiet --release --manifest-path dsp/Cargo.toml --target aarch64-apple-darwin

# The Rust core against the C++ reference: bit-identical output.
PARITY_BIN := build/parity
test-parity: rust
	$(CXX) -std=c++17 -O2 -ffp-contract=off -Wall -IExtension/DSP -ITests/DSP -Idsp/include \
		Tests/Parity/parity.cpp build/rust/libsampling_reverb_dsp.a -o $(PARITY_BIN)
	./$(PARITY_BIN)

test-dsp: $(DSP_TEST_BIN)
	./$(DSP_TEST_BIN)

$(DSP_TEST_BIN): $(wildcard Extension/DSP/*) $(wildcard Tests/DSP/*)
	mkdir -p build
	$(CXX) $(DSP_FLAGS) Tests/DSP/*.cpp -o $@

test-swift: project
	xcodebuild -project $(PROJECT) -scheme SamplingReverb -derivedDataPath $(DERIVED) -quiet test

clean:
	rm -rf build $(PROJECT)
