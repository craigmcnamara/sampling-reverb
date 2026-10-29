#include "SampleEngine.hpp"
#include "Signals.hpp"
#include "TestHarness.hpp"

using namespace sr;

namespace {

constexpr float kSR = 48000.f;

struct EngineRig {
    SampleEngine engine;
    EngineParams params;
    sig::Buffer outL, outR;
    int maxActiveVoices = 0;

    explicit EngineRig(EngineParams p = {}) : params(p) {
        engine.prepare(kSR);
        engine.setParams(params);
    }
    void set(const EngineParams& p) {
        params = p;
        engine.setParams(params);
    }
    void run(const sig::Buffer& in) {
        const size_t base = outL.size();
        outL.resize(base + in.size());
        outR.resize(base + in.size());
        for (size_t i = 0; i < in.size(); i += 256) {
            const int n = static_cast<int>(std::min<size_t>(256, in.size() - i));
            engine.process(&in[i], &in[i], &outL[base + i], &outR[base + i], n);
            engine.advanceLife(n / kSR);
            maxActiveVoices = std::max(maxActiveVoices, engine.activeVoiceCount());
        }
    }
    void wait(float seconds) { run(sig::silence(seconds, kSR)); }
    float seconds() const { return outL.size() / kSR; }

    std::vector<SRSlotVisual> visuals() const {
        std::vector<SRSlotVisual> v(SR_MAX_SLOTS);
        engine.fillVisual(v.data());
        return v;
    }
};

sig::Buffer note(float hz, float seconds = 0.3f, float amp = 0.5f) { return sig::toneBurst(hz, seconds, amp, kSR); }
sig::Buffer hit(uint32_t seed = 1) { return sig::noiseBurst(0.2f, 0.5f, kSR, seed); }

} // namespace

// ---- Analysis --------------------------------------------------------------------------------

TEST_CASE(engine_detects_pitch_of_a_tone) {
    for (float hz : {110.f, 220.f, 440.f, 880.f}) {
        EngineRig rig;
        rig.run(note(hz));
        const Features& f = rig.engine.lastFeatures();
        CHECK_NEAR(f.midi, 69.f + 12.f * std::log2(hz / 440.f), 0.35);
        CHECK(f.clarity > 0.8f);
    }
}

TEST_CASE(engine_noise_is_unpitched_and_bright) {
    EngineRig rig;
    rig.run(hit());
    const Features& f = rig.engine.lastFeatures();
    CHECK(f.midi < 0.f);
    CHECK(f.clarity < 0.5f);
    CHECK(f.bright > 0.7f);
}

TEST_CASE(engine_low_tones_are_darker_than_high_tones) {
    EngineRig low, high;
    low.run(note(110.f));
    high.run(note(1000.f));
    CHECK(low.engine.lastFeatures().bright < high.engine.lastFeatures().bright);
    CHECK(low.engine.lastFeatures().bright < 0.3f);
}

TEST_CASE(engine_map_position_uses_brightness_and_chroma) {
    EngineRig rig;
    rig.run(note(440.f));  // A, chroma 9
    const Features& f = rig.engine.lastFeatures();
    CHECK_NEAR(f.x, f.bright, 1e-6);
    CHECK_NEAR(f.y, 9.5f / 12.f, 0.03);
}

// ---- Capture ---------------------------------------------------------------------------------

TEST_CASE(engine_silence_captures_nothing) {
    EngineRig rig;
    rig.wait(3.f);
    CHECK(rig.engine.poolCount() == 0);
    CHECK(sig::peak(rig.outL) == 0.f);
}

TEST_CASE(engine_novel_sound_is_captured_after_length) {
    EngineParams p;
    p.lengthSec = 0.5f;
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(0.1f);
    CHECK(rig.engine.poolCount() == 0);  // still recording (0.4 s elapsed)
    rig.wait(0.3f);
    CHECK(rig.engine.poolCount() == 1);
    CHECK(rig.engine.totalCaptures() == 1);
    CHECK(rig.engine.totalTriggers() == 0);
}

TEST_CASE(engine_ignores_sounds_below_sensitivity) {
    EngineParams p;
    p.sensitivityDb = -30.f;
    EngineRig rig(p);
    rig.run(note(220.f, 0.3f, 0.01f));  // about -46 dBFS envelope
    rig.wait(1.5f);
    CHECK(rig.engine.poolCount() == 0);
}

TEST_CASE(engine_distinct_sounds_fill_separate_slots) {
    EngineRig rig;
    rig.run(note(220.f));
    rig.wait(1.f);
    rig.run(hit());
    rig.wait(1.f);
    CHECK(rig.engine.poolCount() == 2);
    auto v = rig.visuals();
    int pitched = 0, noisy = 0;
    for (const auto& s : v) {
        if (!s.active) continue;
        (s.clarity > 0.5f ? pitched : noisy)++;
    }
    CHECK(pitched == 1);
    CHECK(noisy == 1);
}

// ---- Matching --------------------------------------------------------------------------------

TEST_CASE(engine_similar_sound_triggers_instead_of_capturing) {
    EngineRig rig;
    rig.run(note(220.f));
    rig.wait(1.f);
    const size_t before = rig.outL.size();
    CHECK(sig::peak(rig.outL) == 0.f);
    rig.run(note(220.f));
    rig.wait(0.5f);
    CHECK(rig.engine.totalTriggers() == 1);
    CHECK(rig.engine.poolCount() == 1);
    CHECK(sig::rms(rig.outL, before, rig.outL.size()) > 1e-3f);
}

TEST_CASE(engine_octave_matches_as_similar) {
    EngineParams p;
    p.harmony = 0.f;
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(1.f);
    rig.run(note(440.f));
    rig.wait(1.f);
    CHECK(rig.engine.totalTriggers() == 1);
}

TEST_CASE(engine_harmony_controls_whether_a_fifth_matches) {
    for (float harmony : {0.f, 1.f}) {
        EngineParams p;
        p.harmony = harmony;
        EngineRig rig(p);
        rig.run(note(220.f));
        rig.wait(1.f);
        rig.run(note(330.f));  // a fifth above
        rig.wait(1.f);
        if (harmony == 0.f) {
            CHECK(rig.engine.totalTriggers() == 0);
            CHECK(rig.engine.poolCount() == 2);
        } else {
            CHECK(rig.engine.totalTriggers() == 1);
            CHECK(rig.engine.poolCount() == 1);
        }
    }
}

TEST_CASE(engine_match_zero_is_strict) {
    EngineParams p;
    p.match = 0.f;
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(1.f);
    rig.run(hit());
    rig.wait(1.f);
    CHECK(rig.engine.totalTriggers() == 0);
}

TEST_CASE(engine_focus_biases_matching_towards_the_crosshair) {
    // Same sound, but the stored point is far from the focus and focus is at full strength.
    EngineParams p;
    p.focusAmount = 1.f;
    p.focusX = 1.f;
    p.focusY = 0.f;
    p.match = 0.25f;
    EngineRig far(p);
    far.run(note(220.f));  // sits near x≈0.2, y≈0.8
    far.wait(1.f);
    far.run(note(220.f));
    far.wait(0.5f);
    CHECK(far.engine.totalTriggers() == 0);

    p.focusX = 0.2f;
    p.focusY = 0.8f;
    EngineRig near(p);
    near.run(note(220.f));
    near.wait(1.f);
    near.run(note(220.f));
    near.wait(0.5f);
    CHECK(near.engine.totalTriggers() == 1);
}

// ---- Retention -------------------------------------------------------------------------------

TEST_CASE(engine_low_retention_discards_unplayed_samples) {
    EngineParams p;
    p.retention = 0.f;  // 3 s lifetime
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(1.f);
    CHECK(rig.engine.poolCount() == 1);
    rig.wait(3.5f);
    CHECK(rig.engine.poolCount() == 0);
}

TEST_CASE(engine_full_retention_keeps_samples_forever) {
    EngineParams p;
    p.retention = 1.f;
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(60.f);
    CHECK(rig.engine.poolCount() == 1);
}

TEST_CASE(engine_playing_a_sample_extends_its_life) {
    EngineParams p;
    p.retention = 0.f;  // 3 s lifetime
    EngineRig rig(p);
    rig.run(note(220.f));       // t=0: captured at ~0.8 s, would die at ~3.8 s
    rig.wait(0.7f);
    rig.run(hit());             // t=1.0: captured at ~1.8 s, dies at ~4.8 s
    rig.wait(1.2f);
    rig.run(note(220.f));       // t=2.5: retrigger extends the tone's life to ~5.7 s
    rig.wait(2.2f);             // t≈5.0
    CHECK(rig.engine.poolCount() == 1);
    auto v = rig.visuals();
    for (const auto& s : v)
        if (s.active) CHECK(s.clarity > 0.5f);  // the survivor is the replayed tone
}

TEST_CASE(engine_full_pool_replaces_the_shortest_lived_slot) {
    EngineParams p;
    p.match = 0.f;  // everything is novel
    p.lengthSec = 0.1f;
    p.retention = 1.f;
    EngineRig rig(p);
    // A chromatic run: neighbouring semitones never match, octaves differ in brightness.
    for (int i = 0; i < SR_MAX_SLOTS + 4; ++i) {
        rig.run(note(110.f * std::pow(2.f, i / 12.f), 0.2f));
        rig.wait(0.1f);
    }
    CHECK(rig.engine.poolCount() == SR_MAX_SLOTS);
    CHECK(rig.engine.totalCaptures() == SR_MAX_SLOTS + 4);
}

// ---- Freeze ----------------------------------------------------------------------------------

TEST_CASE(engine_freeze_blocks_capture_and_pauses_aging) {
    EngineParams p;
    p.retention = 0.f;
    EngineRig rig(p);
    rig.run(note(220.f));
    rig.wait(1.f);
    p.freeze = true;
    rig.set(p);
    rig.run(hit());
    rig.wait(10.f);
    CHECK(rig.engine.poolCount() == 1);
    CHECK(rig.engine.totalCaptures() == 1);
    rig.run(note(220.f));  // matching still works while frozen
    rig.wait(0.5f);
    CHECK(rig.engine.totalTriggers() == 1);
}

// ---- Voices ----------------------------------------------------------------------------------

TEST_CASE(engine_density_caps_simultaneous_voices) {
    for (int density : {1, 4}) {
        EngineParams p;
        p.density = density;
        p.lengthSec = 1.5f;
        EngineRig rig(p);
        rig.run(note(220.f));
        rig.wait(1.5f);
        for (int i = 0; i < 5; ++i) rig.run(note(220.f, 0.2f));
        rig.wait(2.f);
        CHECK(rig.engine.totalTriggers() >= 4);
        CHECK(rig.maxActiveVoices <= density);
        if (density > 1) CHECK(rig.maxActiveVoices > 1);
    }
}

TEST_CASE(engine_visual_state_reports_triggers_and_levels) {
    EngineRig rig;
    rig.run(note(220.f));
    rig.wait(1.f);
    auto before = rig.visuals();
    int slot = -1;
    for (int s = 0; s < SR_MAX_SLOTS; ++s)
        if (before[s].active) slot = s;
    REQUIRE(slot >= 0);
    CHECK(before[slot].generation == 1);
    CHECK(before[slot].triggerCount == 0);
    CHECK(before[slot].level == 0.f);
    CHECK_RANGE(before[slot].life, 0.9f, 1.f);

    rig.run(note(220.f, 0.15f));
    auto during = rig.visuals();
    CHECK(during[slot].triggerCount == 1);
    CHECK(during[slot].level > 0.1f);
    CHECK(during[slot].life > before[slot].life);
}

TEST_CASE(engine_voice_output_is_the_recorded_sound) {
    EngineParams p;
    p.lengthSec = 0.5f;
    EngineRig rig(p);
    rig.run(note(440.f));
    rig.wait(1.f);
    const size_t start = rig.outL.size();
    rig.run(note(440.f));
    // The replayed sample (after the analysis window) is a 440 Hz tone too.
    const size_t from = start + 2048 + sig::frames(0.05f, kSR);
    CHECK_NEAR(sig::zeroCrossingHz(rig.outL, from, from + sig::frames(0.2f, kSR), kSR), 440.f, 10.f);
}
