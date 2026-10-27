//! The official riscv-tests rv32ui suite (github.com/riscv-software-src/riscv-tests).
//!
//! Point `RISCV_TESTS_DIR` at binaries built by `scripts/build-riscv-tests.sh`.
//! Without it the test is skipped, so `cargo test` still works with no RISC-V toolchain.
//!
//! Each test exits with a0 = 0 on success, or a0 = (failing case << 1) | 1.

use std::fs;
use std::path::PathBuf;

#[test]
fn rv32ui_compliance() {
    let Some(dir) = std::env::var_os("RISCV_TESTS_DIR") else {
        eprintln!("skipping rv32ui compliance: RISCV_TESTS_DIR is not set");
        return;
    };

    let mut tests: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("RISCV_TESTS_DIR is not readable")
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_none())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("rv32ui-p-")
        })
        .collect();
    tests.sort();
    assert!(!tests.is_empty(), "no rv32ui-p-* binaries in {dir:?}");

    let mut failures = Vec::new();
    for path in &tests {
        let name = path.file_name().unwrap().to_string_lossy();
        let bytes = fs::read(path).unwrap();
        let outcome = rv32i_sim::load_program(&bytes, 0x8000_0000, 1 << 20)
            .map_err(|e| e.to_string())
            .and_then(|mut cpu| cpu.run(100_000, |_, _| {}).map_err(|e| e.to_string()));
        match outcome {
            Ok(exit) if exit.code == 0 => {
                println!("PASS  {name:<20} {:>6} instructions", exit.steps)
            }
            Ok(exit) => failures.push(format!("FAIL  {name}: test case {} failed", exit.code >> 1)),
            Err(e) => failures.push(format!("FAIL  {name}: {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} rv32ui tests failed:\n{}",
        failures.len(),
        tests.len(),
        failures.join("\n")
    );
}
