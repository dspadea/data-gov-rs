//! Process-level tests for `--color`'s one production call site,
//! `run()`'s `color_mode.apply_as_global_override()` (`tools/cli/ui/mod.rs`).
//!
//! `ColorMode::apply_as_global_override` itself is unit-tested in
//! `tools/cli/ui/colors.rs`, but nothing in the unit suite drives `run()`
//! parsing `--color` off `argv` and applying the *resolved* mode — a
//! mutation replacing that call with a hardcoded
//! `ColorMode::Auto.apply_as_global_override()` (ignoring the flag
//! entirely) passed all 90 tests before this file existed (#58.1).
//!
//! `assert_cmd` always captures stdout as a pipe, never a terminal, which
//! is exactly the scenario `--color always` has to override and `--color
//! auto` would otherwise leave uncolored — so a piped stdout is not a
//! limitation here, it is the point.

use assert_cmd::Command;
use predicates::prelude::*;

/// The ANSI escape introducer (`ESC` = `\x1b`). `colored` always opens a
/// styled run with `\x1b[`, so its presence or absence in captured stdout
/// is a direct, byte-level answer to "did this process actually emit
/// color" — not an assertion about what `ColorHelper` merely *decided*.
const ANSI_ESCAPE: &str = "\u{1b}[";

#[test]
fn color_always_emits_real_ansi_escapes_even_though_stdout_is_piped() {
    let dir = tempfile::tempdir().expect("tempdir");

    Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .args(["--color", "always", "help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ANSI_ESCAPE));
}

#[test]
fn color_never_emits_no_ansi_escapes() {
    let dir = tempfile::tempdir().expect("tempdir");

    Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .args(["--color", "never", "help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ANSI_ESCAPE).not());
}

#[test]
fn no_color_env_overrides_color_always() {
    let dir = tempfile::tempdir().expect("tempdir");

    Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .env("NO_COLOR", "1")
        .args(["--color", "always", "help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(ANSI_ESCAPE).not());
}
