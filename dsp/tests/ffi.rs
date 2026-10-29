//! The C ABI the Audio Unit uses: metadata, null safety and bad indexes.

use std::ffi::CStr;

use sampling_reverb_dsp::ffi::*;
use sampling_reverb_dsp::params::*;

fn text(p: *const std::ffi::c_char) -> &'static str {
    unsafe { CStr::from_ptr(p) }.to_str().unwrap()
}

#[test]
fn metadata_matches_the_parameter_table_and_presets() {
    assert_eq!(sr_param_count() as usize, PARAM_COUNT);
    for spec in &PARAM_SPECS {
        let a = spec.address as u64;
        let s = sr_param_spec(a);
        assert_eq!(text(s.identifier), spec.identifier.to_str().unwrap());
        assert_eq!(text(s.name), spec.name.to_str().unwrap());
        assert_eq!(
            (s.min, s.max, s.def, s.unit, s.exponential),
            (spec.min, spec.max, spec.def, spec.unit as u32, spec.exponential)
        );
        assert_eq!(sr_preset_leaves_alone(a), preset_leaves_alone(a));
        let names = value_names(a);
        assert_eq!(sr_param_value_name_count(a) as usize, names.len());
        for (i, n) in names.iter().enumerate() {
            assert_eq!(text(sr_param_value_name(a, i as i32)), n.to_str().unwrap());
        }
    }
    assert_eq!(sr_param_value_name_count(Param::Grid as u64), 8);
    assert_eq!(text(sr_param_value_name(Param::PreDelayDiv as u64, 11)), "1/4D");
    assert_eq!(sr_preset_count() as usize, PRESET_COUNT);
    for (i, preset) in PRESETS.iter().enumerate() {
        assert_eq!(text(sr_preset_name(i as i32)), preset.name.to_str().unwrap());
        for (p, v) in preset.values.iter().enumerate() {
            assert_eq!(sr_preset_value(i as i32, p as u64), *v);
        }
    }
}

#[test]
fn tolerates_null_pointers_and_bad_indexes() {
    unsafe {
        let null = std::ptr::null();
        sr_kernel_set_parameter(null, 0, 1.0);
        assert_eq!(sr_kernel_get_parameter(null, 0), 0.0);
        sr_kernel_set_bypass(null, true);
        sr_kernel_prepare(null, 48000.0);
        sr_kernel_process(
            null,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            64,
        );
        let out = std::ptr::null_mut();
        assert_eq!(
            sr_kernel_read_visual(null, out, 0, std::ptr::null_mut(), std::ptr::null_mut(), 0, std::ptr::null_mut()),
            0
        );
        assert_eq!(sr_kernel_clock(null).tempo, 120.0);
        sr_kernel_free(std::ptr::null_mut());

        let k = sr_kernel_new();
        sr_kernel_prepare(k, f64::NAN); // ignored
        sr_kernel_prepare(k, -1.0); // ignored
        sr_kernel_prepare(k, 48000.0);
        let mut buf = [0.1f32; 64];
        let p = buf.as_mut_ptr();
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), 0);
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), -5);
        sr_kernel_process(k, std::ptr::null(), p, std::ptr::null(), p, p.add(32), 32);
        sr_kernel_process(k, std::ptr::null(), p, p, p, p.add(32), 32);
        assert_eq!(
            sr_kernel_read_visual(
                k,
                std::ptr::null_mut(),
                99,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                99,
                std::ptr::null_mut()
            ),
            0
        );
        sr_kernel_free(k);
    }
    assert!(sr_param_spec(999).identifier.is_null());
    assert!(sr_param_value_name(Param::Grid as u64, 8).is_null());
    assert!(sr_param_value_name(Param::Grid as u64, -1).is_null());
    assert!(sr_param_value_name(Param::Mix as u64, 0).is_null());
    assert!(sr_preset_name(-1).is_null() && sr_preset_name(7).is_null());
    assert_eq!(sr_preset_value(0, 999), 0.0);
    assert_eq!(sr_preset_value(99, 0), 0.0);
}

#[test]
fn processes_through_the_c_abi_in_place() {
    unsafe {
        let k = sr_kernel_new();
        sr_kernel_prepare(k, 48000.0);
        sr_kernel_set_parameter(k, Param::Mix as u64, 100.0);
        let mut l = vec![0.0f32; 48000];
        let mut r = vec![0.0f32; 48000];
        l[0] = 1.0;
        r[0] = 1.0;
        for i in (0..l.len()).step_by(512) {
            let n = 512.min(l.len() - i) as i32;
            let (pl, pr) = (l.as_mut_ptr().add(i), r.as_mut_ptr().add(i));
            sr_kernel_process(k, std::ptr::null(), pl, pr, pl, pr, n);
        }
        assert!(l.iter().all(|x| x.is_finite()));
        assert!(l[9600..19200].iter().any(|x| x.abs() > 1e-4), "a reverb tail");
        sr_kernel_free(k);
    }
}
