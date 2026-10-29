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

// ---- Attack ----------------------------------------------------------------------------------

namespace {

// A sustained note: 5 ms attack, then constant level, so the replayed sample's own shape is flat.
sig::Buffer pad(float hz, float seconds) {
    sig::Buffer b = sig::sine(hz, seconds, 0.4f, kSR);
    const size_t ramp = sig::frames(0.005f, kSR);
    for (size_t i = 0; i < ramp; ++i) b[i] *= static_cast<float>(i) / ramp;
    return b;
}

// Replays a captured pad and returns voice RMS just after the voice starts vs. 400–450 ms later.
std::pair<float, float> voiceOnsetLevels(float attackSec) {
    EngineParams p;
    p.lengthSec = 1.f;
    p.attackSec = attackSec;
    EngineRig rig(p);
    rig.run(pad(220.f, 0.9f));
    rig.wait(1.f);
    const size_t start = rig.outL.size();
    rig.run(pad(220.f, 0.9f));
    size_t voice = start;
    while (voice < rig.outL.size() && rig.outL[voice] == 0.f) ++voice;
    REQUIRE(voice < rig.outL.size());
    return {sig::rms(rig.outL, voice, voice + sig::frames(0.03f, kSR)),
            sig::rms(rig.outL, voice + sig::frames(0.4f, kSR), voice + sig::frames(0.45f, kSR))};
}

} // namespace

TEST_CASE(engine_short_attack_plays_samples_at_full_level_immediately) {
    const auto [early, late] = voiceOnsetLevels(0.001f);
    CHECK(early > 0.5f * late);
}

TEST_CASE(engine_long_attack_swells_samples_in) {
    const auto [early, late] = voiceOnsetLevels(0.5f);
    if (!(early < 0.1f * late)) std::printf("    early %g, late %g\n", early, late);
    CHECK(early < 0.1f * late);
    CHECK(late > 1e-3f);
}

// ---- Long samples ----------------------------------------------------------------------------

TEST_CASE(engine_captures_and_replays_long_samples) {
    EngineParams p;
    p.lengthSec = 15.f;
    p.attackSec = 0.001f;
    EngineRig rig(p);
    rig.run(pad(220.f, 14.f));  // a long sustained note
    rig.wait(2.f);
    REQUIRE(rig.engine.poolCount() == 1);
    const size_t start = rig.outL.size();
    rig.run(note(220.f));
    rig.wait(14.f);
    // The replay is still sounding 12 s in: the whole long take was kept.
    const size_t at = start + sig::frames(12.f, kSR);
    CHECK(sig::rms(rig.outL, at, at + sig::frames(0.5f, kSR)) > 0.05f);
    CHECK_NEAR(sig::zeroCrossingHz(rig.outL, at, at + sig::frames(0.5f, kSR), kSR), 220.f, 5.f);
}

TEST_CASE(engine_long_lengths_still_capture_overlapping_phrases) {
    // With a 5 s Length, three distinct notes half a second apart all get recorded.
    EngineParams p;
    p.lengthSec = 5.f;
    p.match = 0.f;
    EngineRig rig(p);
    for (float hz : {220.f, 330.f, 494.f}) {
        rig.run(note(hz, 0.3f));
        rig.wait(0.2f);
    }
    rig.wait(6.f);
    CHECK(rig.engine.poolCount() == 3);
    CHECK(rig.engine.totalCaptures() == 3);
}

TEST_CASE(engine_captured_samples_fade_in_and_out_without_clicks) {
    EngineParams p;
    p.lengthSec = 0.5f;
    p.attackSec = 0.0005f;
    EngineRig rig(p);
    rig.run(pad(220.f, 0.8f));  // still loud when the 0.5 s capture ends
    rig.wait(1.f);
    const size_t start = rig.outL.size();
    rig.run(pad(220.f, 0.8f));
    rig.wait(0.5f);
    size_t first = start, last = 0;
    while (first < rig.outL.size() && rig.outL[first] == 0.f) ++first;
    for (size_t i = first; i < rig.outL.size(); ++i)
        if (rig.outL[i] != 0.f) last = i;
    REQUIRE(last > first);
    const float middle = sig::rms(rig.outL, (first + last) / 2, (first + last) / 2 + 2400);
    CHECK(std::fabs(rig.outL[first]) < 0.05f * middle);  // starts from silence
    CHECK(std::fabs(rig.outL[last]) < 0.05f * middle);   // ends at silence, no cut-off click
}

TEST_CASE(engine_one_sustained_note_is_captured_once) {
    EngineParams p;
    p.lengthSec = 5.f;
    EngineRig rig(p);
    rig.run(pad(220.f, 3.f));
    rig.wait(3.f);
    CHECK(rig.engine.totalCaptures() == 1);
    CHECK(rig.engine.poolCount() == 1);
}

TEST_CASE(engine_repeating_a_note_during_its_long_capture_does_not_duplicate_it) {
    EngineParams p;
    p.lengthSec = 10.f;
    EngineRig rig(p);
    for (int i = 0; i < 3; ++i) {
        rig.run(note(220.f, 0.3f));
        rig.wait(0.7f);
    }
    rig.wait(9.f);
    CHECK(rig.engine.totalCaptures() == 1);
}

// ---- Smooth endings --------------------------------------------------------------------------

TEST_CASE(engine_density_steals_voices_with_a_gentle_release) {
    // Two long samples at different pitches; with Density 1, triggering the second must fade the
    // first out gently rather than chopping it.
    EngineParams p;
    p.lengthSec = 3.f;
    p.density = 1;
    p.attackSec = 0.001f;
    EngineRig rig(p);
    rig.run(pad(220.f, 2.5f));
    rig.wait(1.f);
    rig.run(pad(660.f, 2.5f));
    rig.wait(1.f);
    REQUIRE(rig.engine.poolCount() == 2);

    rig.run(note(220.f));  // plays the 220 Hz sample
    rig.wait(0.5f);
    const uint64_t before = rig.engine.totalTriggers();
    size_t steal = 0;
    sig::Buffer hit = note(660.f);
    for (size_t i = 0; i < hit.size() && steal == 0; i += 256) {  // find the block that triggers it
        const size_t n = std::min<size_t>(256, hit.size() - i);
        rig.run(sig::Buffer(hit.begin() + i, hit.begin() + i + n));
        if (rig.engine.totalTriggers() > before) steal = rig.outL.size();
    }
    REQUIRE(steal > 0);
    rig.wait(0.5f);

    const float held = sig::toneLevel(rig.outL, steal - sig::frames(0.06f, kSR), steal - sig::frames(0.01f, kSR), 220.f, kSR);
    const float after = sig::toneLevel(rig.outL, steal + sig::frames(0.02f, kSR), steal + sig::frames(0.07f, kSR), 220.f, kSR);
    if (!(after > 0.3f * held)) std::printf("    220 Hz level before steal %g, 20-70 ms after %g\n", held, after);
    CHECK(held > 0.05f);
    CHECK(after > 0.3f * held);
}

TEST_CASE(engine_samples_cut_mid_note_fade_out_gradually) {
    // A note still sounding when Length ends: the replay should taper over a meaningful stretch,
    // not drop out in the final few milliseconds.
    EngineParams p;
    p.lengthSec = 1.f;
    p.attackSec = 0.001f;
    EngineRig rig(p);
    rig.run(pad(220.f, 1.5f));
    rig.wait(1.f);
    const size_t start = rig.outL.size();
    rig.run(note(220.f));
    rig.wait(1.5f);
    size_t first = start, last = 0;
    while (first < rig.outL.size() && rig.outL[first] == 0.f) ++first;
    for (size_t i = first; i < rig.outL.size(); ++i)
        if (rig.outL[i] != 0.f) last = i;
    REQUIRE(last > first + sig::frames(0.8f, kSR));
    const size_t mid = (first + last) / 2;
    const float middle = sig::rms(rig.outL, mid, mid + sig::frames(0.05f, kSR));
    const float nearEnd = sig::rms(rig.outL, last - sig::frames(0.15f, kSR), last - sig::frames(0.10f, kSR));
    if (!(nearEnd < 0.7f * middle)) std::printf("    middle %g, 100-150 ms before the end %g\n", middle, nearEnd);
    CHECK(nearEnd < 0.7f * middle);
}

// ---- Quantize --------------------------------------------------------------------------------

namespace {

constexpr double kBeatsPerFrame120 = 120.0 / (60.0 * kSR);

// Feeds `in` one frame at a time and returns the frame (counted from the engine's start) at which
// the next trigger fired, or -1.
long long triggerFrame(SampleEngine& engine, const sig::Buffer& in, long long& clock) {
    const uint64_t before = engine.totalTriggers();
    long long fired = -1;
    float l, r;
    for (size_t i = 0; i < in.size(); ++i, ++clock) {
        engine.process(&in[i], &in[i], &l, &r, 1);
        if (fired < 0 && engine.totalTriggers() != before) fired = clock;
    }
    return fired;
}

struct QuantizeRig {
    SampleEngine engine;
    long long clock = 0;

    QuantizeRig(bool quantize, double gridBeats, double barOrigin = 0.0) {
        EngineParams p;
        p.lengthSec = 0.5f;
        p.quantize = quantize;
        p.gridBeats = gridBeats;
        engine.prepare(kSR);
        engine.setParams(p);
        engine.setClock({0.0, kBeatsPerFrame120, barOrigin});  // the engine extrapolates from here
        // Capture a note, then let it land in the pool.
        triggerFrame(engine, note(440.f), clock);
        triggerFrame(engine, sig::silence(1.f, kSR), clock);
    }

    // Plays the same note `offset` frames after the next whole second; returns when it triggered
    // and when it started.
    std::pair<long long, long long> replay(long long offset) {
        const long long target = (clock / 48000 + 1) * 48000 + offset;
        triggerFrame(engine, sig::silence(static_cast<float>(target - clock) / kSR, kSR), clock);
        const long long onset = clock;
        return {triggerFrame(engine, note(440.f), clock), onset};
    }
};

} // namespace

TEST_CASE(engine_unquantized_trigger_fires_right_after_analysis) {
    QuantizeRig rig(false, 0.5);
    REQUIRE(rig.engine.poolCount() == 1);
    const auto [fired, onset] = rig.replay(3000);
    REQUIRE(fired >= 0);
    CHECK(fired - onset < 2048 + 64);
}

TEST_CASE(engine_quantize_holds_the_trigger_until_the_next_grid_point) {
    // 1/8 notes at 120 BPM are 12000 frames apart.
    QuantizeRig rig(true, 0.5);
    REQUIRE(rig.engine.poolCount() == 1);
    for (long long offset : {3000LL, 9500LL, 13000LL}) {
        const auto [fired, onset] = rig.replay(offset);
        REQUIRE(fired >= 0);
        if (fired % 12000 > 1) std::printf("    offset %lld: fired at %lld\n", offset, fired);
        CHECK(fired % 12000 <= 1);
        CHECK(fired > onset + 2048);
        CHECK(fired <= onset + 2048 + 12000);
    }
}

TEST_CASE(engine_quantize_supports_triplets_and_follows_the_bar_origin) {
    // 1/8 triplets (1/3 beat = 8000 frames), with the bar line a quarter beat (6000 frames) in.
    QuantizeRig rig(true, 1.0 / 3.0, 0.25);
    const auto [fired, onset] = rig.replay(1000);
    REQUIRE(fired >= 0);
    CHECK(((fired - 6000) % 8000 + 8000) % 8000 <= 1);
    CHECK(fired > onset + 2048);
}

TEST_CASE(engine_quantized_slot_is_not_triggered_twice_for_one_grid_point) {
    // A whole bar (4 beats = 96000 frames): a second matching note while the first waits must not
    // queue the same slot again.
    QuantizeRig rig(true, 4.0);
    const uint64_t before = rig.engine.totalTriggers();
    sig::Buffer twoNotes = note(440.f, 0.2f);
    sig::append(twoNotes, sig::silence(0.15f, kSR));
    sig::append(twoNotes, note(440.f, 0.2f));
    sig::append(twoNotes, sig::silence(2.f, kSR));
    triggerFrame(rig.engine, twoNotes, rig.clock);
    CHECK(rig.engine.totalTriggers() - before == 1);
}
