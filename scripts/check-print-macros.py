#!/usr/bin/env python3
"""Fail if the CLI writes user-facing output with `println!` or `eprintln!`.

`println!` panics when the write fails, so piping the CLI into a reader that
quits early - `data-gov list organizations | head -5` - killed the process with
exit 101 and a backtrace note over ordinary Unix usage (#115). The fix routes
every user-facing line through `outln!` and `errln!` in
`data-gov/tools/cli/ui/output.rs`, which recognise a closed pipe and stop
quietly.

That fix holds only while it is complete. One `println!` added later reopens
the defect on whichever command it prints from, and nothing else would notice:
the code compiles, the tests pass, and the crash appears only when somebody
pipes that particular command into `head`. So the absence of the bare macros is
checked here rather than remembered.

Comment lines are exempt, because `output.rs` documents the macros it replaces
by name.

Run with --self-test to check the matcher itself against the cases below. The
gate runs that first, so a change that breaks the matcher fails immediately
rather than silently passing everything.
"""

from __future__ import annotations

import re
import subprocess
import sys

# The four std printing macros, each of which panics on a write error. The
# lookbehind keeps `my_println!` and similar from matching on their tail.
PRINT_MACRO_RE = re.compile(r"(?<!\w)e?print(?:ln)?!")

# Only the CLI binary is covered. The library crates do not print to the
# user's terminal; they report through `data_gov::ui::StatusReporter`.
SCANNED_PREFIX = "data-gov/tools/cli/"

SELF_TEST_CASES: list[tuple[str, bool]] = [
    # (line, is it a violation?)
    ('    println!("Government organizations:");', True),
    ('    eprintln!("Error: {err}");', True),
    ('    print!("no newline");', True),
    ('    eprint!("no newline");', True),
    ("    println!();", True),
    # The replacements, which are the whole point.
    ('    outln!("Government organizations:");', False),
    ('    errln!("Error: {err}");', False),
    ("    outln!();", False),
    # Documentation naming the macros it replaces.
    ("//! `println!` panics when the write fails.", False),
    ("/// Drop-in replacement for `eprintln!`.", False),
    ("    /// be written with `eprintln!` - user-facing error output", False),
    # Identifiers that merely end in a covered name.
    ('    my_println!("x");', False),
    ('    sprint!("x");', False),
]


def is_violation(line: str) -> bool:
    """True if `line` calls a bare std printing macro.

    A line whose first non-space characters are `//` is documentation, and
    documentation is allowed to name the macros.
    """
    if line.lstrip().startswith("//"):
        return False
    return PRINT_MACRO_RE.search(line) is not None


def self_test() -> int:
    failures = 0
    for line, expected in SELF_TEST_CASES:
        actual = is_violation(line)
        if actual != expected:
            failures += 1
            print(
                f"self-test: {line!r} -> {actual}, expected {expected}",
                file=sys.stderr,
            )
    if failures:
        print(f"{failures} self-test case(s) failed.", file=sys.stderr)
        return 1
    print(f"check-print-macros self-test: {len(SELF_TEST_CASES)} cases pass.")
    return 0


def tracked_cli_sources() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", f"{SCANNED_PREFIX}*.rs"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [path for path in out.stdout.splitlines() if path]


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()

    sources = tracked_cli_sources()
    if not sources:
        print(
            f"::error::no tracked .rs files under {SCANNED_PREFIX}. The CLI "
            "moved, so this check is scanning nothing - point it at the new "
            "location.",
            file=sys.stderr,
        )
        return 1

    findings = 0
    for path in sources:
        with open(path, encoding="utf-8") as handle:
            for number, line in enumerate(handle, start=1):
                if is_violation(line):
                    findings += 1
                    print(f"{path}:{number}: {line.rstrip()}", file=sys.stderr)

    if findings:
        print(
            f"\n{findings} bare printing macro(s) above. `println!` and "
            "`eprintln!` panic when the reader closes the pipe (#115). Use "
            "`outln!` and `errln!` from `data-gov/tools/cli/ui/output.rs` "
            "instead.",
            file=sys.stderr,
        )
        return 1

    print(f"check-print-macros: {len(sources)} CLI source file(s) clean.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
