use std::process::Command;

use hesper::{DEMO_DONE, DemoError, run_demo};
use hesper_cpu6502::StepKind;

#[test]
fn demo_executes_from_reset_to_completion_and_writes_all_ten_bytes() {
    let mut trace = Vec::new();
    let result = run_demo(54, |step, total| trace.push((*step, total))).unwrap();
    assert_eq!(
        &result.ram.as_slice()[0x0200..0x020a],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    );
    assert_eq!(result.ram.as_slice()[0x01ff], 0);
    assert_eq!(result.ram.as_slice()[0x020a], 0);
    assert_eq!(&result.ram.as_slice()[0xfffc..0xfffe], &[0x00, 0x80]);
    assert_eq!(result.steps, 54);
    assert_eq!(result.instruction_cycles, 147);
    assert_eq!(result.reset_cycles, 7);
    let state = result.registers;
    assert_eq!(
        (
            state.a,
            state.x,
            state.y,
            state.sp,
            state.pc,
            state.status.bits()
        ),
        (9, 10, 0, 0xff, 0x800f, 0x27)
    );
    assert_eq!(trace.len(), 54);
    assert_eq!(
        (
            trace[0].0.address,
            trace[0].0.kind,
            trace[0].0.before.sp,
            trace[0].1
        ),
        (0x8000, StepKind::Instruction { opcode: 0xd8 }, 0xfd, 9)
    );
    assert_eq!(
        (
            trace[53].0.address,
            trace[53].0.kind,
            trace[53].0.cycles,
            trace[53].1
        ),
        (0x800d, StepKind::Instruction { opcode: 0xd0 }, 2, 154)
    );
    assert!(trace.iter().all(|(step, _)| step.address != DEMO_DONE));
}

#[test]
fn demo_budget_zero_and_one_short_fail_with_context() {
    for (limit, expected_pc) in [(0, 0x8000), (53, 0x800d)] {
        let mut calls = 0;
        match run_demo(limit, |_, _| calls += 1) {
            Err(DemoError::StepLimit { limit: actual, pc }) => {
                assert_eq!(actual, limit);
                assert_eq!(pc, expected_pc);
            }
            _ => panic!("expected a step-limit error"),
        }
        assert_eq!(calls, limit);
    }
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hesper"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_prints_cpu_results() {
    let result = cli(&[]);
    assert!(result.status.success());
    assert!(result.stderr.is_empty());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap(),
        "$0200..$0209: 0 1 2 3 4 5 6 7 8 9\nA=09 X=0A Y=00 SP=FF PC=800F P=27\nCompleted: 54 instructions, 147 instruction cycles + 7 reset cycles = 154 total cycles\n"
    );
}

#[test]
fn cli_trace_has_before_after_and_instruction_and_running_total() {
    let result = cli(&["--trace", "--max-steps", "54"]);
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert_eq!(
        output.lines().filter(|line| line.contains(" -> ")).count(),
        54
    );
    assert!(output.starts_with("$8000 D8 | A=00 X=00 Y=00 SP=FD PC=8000 P=24 -> A=00 X=00 Y=00 SP=FD PC=8001 P=24 | +2 cycles total=9\n"));
    assert!(output.contains("$800D D0 | A=09 X=0A Y=00 SP=FF PC=800D P=27 -> A=09 X=0A Y=00 SP=FF PC=800F P=27 | +2 cycles total=154"));
}

#[test]
fn cli_reports_step_limit_and_invalid_arguments() {
    for (args, message) in [
        (vec!["--max-steps", "0"], "step limit 0 reached at $8000"),
        (vec!["--max-steps", "53"], "step limit 53 reached at $800D"),
        (
            vec!["--max-steps"],
            "--max-steps requires an unsigned integer",
        ),
        (
            vec!["--max-steps", "-1"],
            "--max-steps requires an unsigned integer",
        ),
        (
            vec!["--max-steps", "no"],
            "--max-steps requires an unsigned integer",
        ),
        (
            vec!["--max-steps", "18446744073709551616"],
            "--max-steps requires an unsigned integer",
        ),
        (vec!["--wat"], "unknown argument: --wat; use --help"),
    ] {
        let result = cli(&args);
        assert!(!result.status.success());
        assert_eq!(
            String::from_utf8(result.stderr).unwrap(),
            format!("hesper: {message}\n")
        );
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn cli_help_does_not_execute_demo() {
    let result = cli(&["--help"]);
    assert!(result.status.success());
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.starts_with("Usage: hesper"));
    assert!(!output.contains("Completed"));
}

#[test]
fn cli_bus_trace_is_bounded_and_includes_reset_without_extra_execution() {
    let full = cli(&["--bus-trace", "--trace-limit", "200"]);
    assert!(full.status.success());
    let output = String::from_utf8(full.stdout).unwrap();
    assert_eq!(
        output
            .lines()
            .filter(|l| l.starts_with('C') && !l.starts_with("Completed"))
            .count(),
        154
    );
    assert!(output.starts_with("C000001 R $0000=00 SYNC=true stalled=false"));
    assert!(output.contains("C000007 R $FFFD=80"));
    assert!(output.contains("$0200..$0209: 0 1 2 3 4 5 6 7 8 9"));
    let tail = cli(&["--bus-trace", "--trace-limit", "2"]);
    assert!(tail.status.success());
    let output = String::from_utf8(tail.stdout).unwrap();
    assert!(output.starts_with("C000153 R $800D=D0 SYNC=true"));
    assert_eq!(output.lines().count(), 5);
    let failure = cli(&["--trace", "--trace-limit", "1", "--max-steps", "53"]);
    assert!(!failure.status.success());
    let output = String::from_utf8(failure.stdout).unwrap();
    assert_eq!(output.lines().count(), 1);
    assert!(output.starts_with("$800B E0 |"));
    for args in [
        vec!["--trace-limit"],
        vec!["--trace-limit", "0"],
        vec!["--trace-limit", "4097"],
        vec!["--trace-limit", "no"],
    ] {
        let output = cli(&args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("--trace-limit requires 1..4096")
        );
    }
}
