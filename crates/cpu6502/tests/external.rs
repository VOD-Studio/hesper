#[path = "support/singlestep.rs"]
mod singlestep;

use singlestep::{FIXTURES, Options, execute_case, parse_cases, run, verify_hash};
use std::path::Path;

#[test]
fn original_nmos_fixtures_cover_21_opcodes_and_672_cases() {
    let report = run(Path::new(FIXTURES), Options::default()).unwrap();
    assert_eq!((report.files, report.cases), (21, 672));
    assert_eq!(report.revision, "2f6980a2d95757486c7bee24355c360e40e2a224");
    assert_eq!(
        report.selection,
        "first 32 cases in original order per listed opcode"
    );
}

#[test]
fn replay_selects_one_case_and_rejects_empty_or_ambiguous_selection() {
    let report = run(
        Path::new(FIXTURES),
        Options {
            opcode: Some(0x69),
            case_index: Some(0),
        },
    )
    .unwrap();
    assert_eq!((report.files, report.cases), (1, 1));
    for options in [
        Options {
            opcode: Some(0x02),
            case_index: None,
        },
        Options {
            opcode: Some(0x69),
            case_index: Some(32),
        },
        Options {
            opcode: None,
            case_index: Some(0),
        },
    ] {
        assert!(run(Path::new(FIXTURES), options).is_err());
    }
    assert!(run(&Path::new(FIXTURES).join("missing"), Options::default()).is_err());
}

#[test]
fn invalid_data_cannot_silently_truncate_or_run_zero_cases() {
    let original = include_bytes!("data/singlestep/ea.json");
    for data in [b"[]".as_slice(), b"[", b"null"] {
        assert!(parse_cases(data, 32).is_err());
    }
    assert!(parse_cases(original, 31).is_err());
    assert!(
        verify_hash(original, &"0".repeat(64))
            .unwrap_err()
            .contains("SHA-256")
    );
    for (field, value) in [("a", 256), ("pc", 65536), ("s", -1)] {
        let mut json: serde_json::Value = serde_json::from_slice(original).unwrap();
        json[0]["initial"][field] = value.into();
        assert!(parse_cases(&serde_json::to_vec(&json).unwrap(), 32).is_err());
    }
    let mut json: serde_json::Value = serde_json::from_slice(original).unwrap();
    let ram = json[0]["initial"]["ram"].as_array_mut().unwrap();
    ram.push(ram[0].clone());
    assert!(
        parse_cases(&serde_json::to_vec(&json).unwrap(), 32)
            .unwrap_err()
            .contains("duplicate RAM")
    );
    json[0]["initial"]["ram"] = serde_json::json!([]);
    assert!(
        parse_cases(&serde_json::to_vec(&json).unwrap(), 32)
            .unwrap_err()
            .contains("missing opcode")
    );
}

#[test]
fn diagnostics_detect_register_memory_flag_and_cycle_mismatches() {
    let cases = parse_cases(include_bytes!("data/singlestep/ea.json"), 32).unwrap();
    let original = &cases[0];
    execute_case(original, 0xea).unwrap();
    // Deliberately corrupt copies to test the checker, never the stored oracle.
    let mut bad = original.clone();
    bad.final_state.a ^= 1;
    assert!(execute_case(&bad, 0xea).unwrap_err().starts_with("A:"));
    let mut bad = original.clone();
    bad.final_state.p ^= 0x80;
    assert!(execute_case(&bad, 0xea).unwrap_err().starts_with("P "));
    let mut bad = original.clone();
    bad.final_state.ram[0].1 ^= 1;
    assert!(execute_case(&bad, 0xea).unwrap_err().starts_with("RAM"));
    let mut bad = original.clone();
    bad.cycles.pop();
    assert!(execute_case(&bad, 0xea).unwrap_err().starts_with("cycles:"));
}
