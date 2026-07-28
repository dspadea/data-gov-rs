//! Process-level tests for `run_cli_mode`'s dispatch between a known
//! command, a shebang script file, and "Unknown command" (#64).
//!
//! All four `run_script_file` unit tests in `tools/cli/ui/mod.rs` call
//! that function directly with a path already known to be a script; none
//! exercise `run_cli_mode`'s decision of *when* to call it. Two distinct
//! defects live in that gap:
//!
//! - Forcing the "is it a file" branch to never fire (the original
//!   coverage gap) passed all 90 tests, so the shebang feature's own
//!   entry point had zero coverage.
//! - The dispatch actually shipped, gated on "did parsing fail" rather
//!   than "is the first token unrecognized" (a second-review finding): a
//!   file named after a known command (`cd`, `ls`, `lcd`, `next`, `show`,
//!   ...) silently shadowed that command whenever its own arguments
//!   failed to parse — `./dg cd` with a file named `cd` in the cwd ran
//!   the file instead of reporting "Usage: cd <path>".
//!
//! Both directions are covered below: a known command must win even when
//! its own arguments are bad, and an unrecognized token must still fall
//! back to running a same-named file.

use assert_cmd::Command;
use std::fs;
#[cfg(unix)]
use std::io::Write;

/// A known command invoked with arguments that fail *that command's own*
/// validation (not "first token unrecognized"), paired with the substring
/// its usage error must contain. Covers the alias families the review
/// named: cd/select, ls/list, lcd/setdir, next, show.
const KNOWN_COMMANDS_WITH_BAD_ARGS: &[(&str, &[&str], &str)] = &[
    ("cd", &[], "Usage: cd"),
    ("select", &[], "Usage: cd"),
    ("ls", &["a", "b"], "Usage: ls"),
    ("list", &["a", "b"], "Usage: ls"),
    ("lcd", &[], "Usage: lcd"),
    ("setdir", &[], "Usage: lcd"),
    ("next", &["extra"], "Usage: next"),
    ("show", &["a", "b"], "Usage: show"),
];

#[test]
fn known_command_with_bad_args_reports_its_own_usage_error_even_with_a_same_named_file_present() {
    // Verified across the whole alias set (CLAUDE.md: "verify across the
    // whole set, never one instance"), not one hand-picked command — the
    // bug was a property of the dispatch gate, not of any single command.
    for (command, args, expected_usage_substring) in KNOWN_COMMANDS_WITH_BAD_ARGS {
        let dir = tempfile::tempdir().expect("tempdir");
        // A file literally named after the command sits in the cwd,
        // containing a line that — if wrongly executed as a script —
        // produces detectable, unrelated output ("info" prints the
        // "Client Information" panel and exits 0).
        fs::write(dir.path().join(command), "info\n").expect("write decoy file");

        let assert = Command::cargo_bin("data-gov")
            .expect("data-gov binary must build")
            .current_dir(dir.path())
            .arg(*command)
            .args(*args)
            .assert();

        let output = assert.get_output();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert_ne!(
            output.status.code(),
            Some(0),
            "'{command}' with bad args must fail, not silently run the same-named \
             file: stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stderr.contains(expected_usage_substring),
            "'{command}': expected stderr to contain {expected_usage_substring:?}, got {stderr:?}"
        );
        assert!(
            !stdout.contains("Client Information"),
            "'{command}': the decoy file must never have been executed as a \
             script, but its output appeared: {stdout:?}"
        );
    }
}

#[test]
fn file_named_search_does_not_shadow_the_search_command() {
    // The precedence rule named explicitly in the review: `search` with
    // no query fails `ReplCommand::from_parts`'s own validation ("Usage:
    // search <query> [limit]") — it must still win over a file named
    // "search" sitting right there in the working directory.
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(dir.path().join("search"), "info\n").expect("write decoy file");

    let assert = Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .current_dir(dir.path())
        .arg("search")
        .assert();

    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_ne!(
        output.status.code(),
        Some(0),
        "stdout={stdout:?} stderr={stderr:?}"
    );
    assert!(stderr.contains("Usage: search"), "stderr was: {stderr:?}");
    assert!(
        !stdout.contains("Client Information"),
        "the file named 'search' must not have run as a script: {stdout:?}"
    );
}

#[test]
fn unrecognized_token_naming_a_file_still_runs_it_as_a_script() {
    // The other direction, so the fix for the shadowing bug does not
    // overcorrect into never running a script at all: a token that names
    // NO known command, but does name a real file, must fall back to
    // running it (#64's actual feature).
    let dir = tempfile::tempdir().expect("tempdir");
    let script_path = dir.path().join("my-report");
    fs::write(&script_path, "info\nquit\n").expect("write script");

    let assert = Command::cargo_bin("data-gov")
        .expect("data-gov binary must build")
        .arg(script_path.to_str().expect("utf8 path"))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("Client Information"),
        "the script's 'info' line must have run: {stdout:?}"
    );
}

/// Exercises the actual `#!/usr/bin/env data-gov` launch end to end: the
/// kernel execs `/usr/bin/env`, which resolves `data-gov` via `PATH` and
/// execs it with the script's own path as `argv[1]` — exactly what
/// `run_cli_mode`'s doc comment claims to support. Every other test in
/// this file drives the same fallback by invoking `data-gov <path>`
/// directly; only this one proves the kernel-level mechanism that gets a
/// shebang script there in the first place actually works. Unix-only:
/// `#!` interpretation is a POSIX kernel feature with no Windows
/// equivalent, and the shebang feature itself is documented as such.
#[test]
#[cfg(unix)]
fn shebang_script_runs_through_the_real_kernel_mechanism() {
    use std::os::unix::fs::PermissionsExt;

    let bin_path = assert_cmd::cargo::cargo_bin("data-gov");
    let bin_dir = bin_path
        .parent()
        .expect("the data-gov binary path has a parent directory")
        .to_path_buf();

    let dir = tempfile::tempdir().expect("tempdir");
    let script_path = dir.path().join("report.sh");
    {
        let mut file = fs::File::create(&script_path).expect("create script file");
        writeln!(file, "#!/usr/bin/env data-gov").expect("write shebang line");
        writeln!(file, "info").expect("write script body");
        writeln!(file, "quit").expect("write script body");
    }
    let mut perms = fs::metadata(&script_path)
        .expect("read script metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).expect("chmod +x the script");

    // `env` resolves the bare `data-gov` named in the shebang via PATH,
    // so the freshly-built binary's directory has to be on it.
    let existing_path = std::env::var_os("PATH").unwrap_or_default();
    let mut new_path = std::ffi::OsString::from(bin_dir);
    new_path.push(":");
    new_path.push(existing_path);

    let output = std::process::Command::new(&script_path)
        .env("PATH", new_path)
        .output()
        .expect("exec the shebang script directly, the way a shell would");

    assert!(
        output.status.success(),
        "shebang launch must succeed: stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Client Information"),
        "the script's 'info' line must have run: {stdout:?}"
    );
}
