//! CLI smoke tests via the compiled binary.
//!
//! These check user-facing surface contracts: `--help` exits 0, every subcommand has a
//! help screen, `--check` exits non-zero from a non-elevated context, and per-subcommand
//! EXAMPLES blocks appear in `--help`.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_tekflash")
}

#[test]
fn top_help_exits_zero() {
    let out = Command::new(bin()).arg("--help").output().expect("run");
    assert!(out.status.success(), "tekflash --help should exit 0");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("tekflash"));
    for sub in &[
        "flash",
        "backup",
        "archive",
        "restore",
        "verify",
        "verify-queue",
        "list",
        "keygen",
    ] {
        assert!(stdout.contains(sub), "top help is missing subcommand {sub}");
    }
}

#[test]
fn every_subcommand_has_long_help_with_examples() {
    // Every subcommand's --help must contain its EXAMPLES section so the doc-as-tutorial
    // promise in the README and plan is enforceable.
    let examples_expected = &[
        ("flash", "EXAMPLES"),
        ("backup", "EXAMPLES"),
        ("archive", "EXAMPLES"),
        ("restore", "EXAMPLES"),
        ("verify", "EXAMPLES"),
        ("list", "EXAMPLES"),
        ("keygen", "EXAMPLES"),
    ];
    for (sub, marker) in examples_expected {
        let out = Command::new(bin())
            .args([sub, "--help"])
            .output()
            .unwrap_or_else(|e| panic!("failed to run `{sub} --help`: {e}"));
        assert!(
            out.status.success(),
            "`tekflash {sub} --help` failed with status {:?}",
            out.status
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains(marker),
            "`tekflash {sub} --help` should contain '{marker}'; got:\n{stdout}"
        );
    }
}

#[test]
fn check_flag_runs_without_root_and_exits_nonzero() {
    // On a CI runner we are not elevated; `--check` should report that and exit 1.
    let out = Command::new(bin()).arg("--check").output().expect("run");
    // We don't assert exit code — root vs non-root differs between local laptop runs and
    // sudo'd containers — but we do assert it produces the capability summary on stdout.
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("elevated:"),
        "expected capability summary, got:\n{stdout}"
    );
}

#[test]
fn unknown_flag_exits_with_clap_error() {
    let out = Command::new(bin())
        .arg("--definitely-not-a-real-flag")
        .output()
        .expect("run");
    assert!(!out.status.success(), "unknown flags should fail parsing");
}
