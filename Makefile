PROJECT      := SamplingReverb.xcodeproj
CONFIG       ?= Release
DERIVED      := build/DerivedData
APP          := $(DERIVED)/Build/Products/$(CONFIG)/Sampling Reverb.app
INSTALL_DIR  ?= /Applications

CXX          ?= clang++
DSP_FLAGS    := -std=c++17 -O2 -Wall -Wextra -IExtension/DSP -ITests/DSP
DSP_TEST_BIN := build/dsp_tests

.PHONY: all project app install validate run test test-dsp test-swift clean

all: app

project:
	xcodegen generate --quiet

app: project
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

test: test-dsp test-swift

test-dsp: $(DSP_TEST_BIN)
	./$(DSP_TEST_BIN)

$(DSP_TEST_BIN): $(wildcard Extension/DSP/*) $(wildcard Tests/DSP/*)
	mkdir -p build
	$(CXX) $(DSP_FLAGS) Tests/DSP/*.cpp -o $@

test-swift: project
	xcodebuild -project $(PROJECT) -scheme SamplingReverb -derivedDataPath $(DERIVED) -quiet test

clean:
	rm -rf build $(PROJECT)
