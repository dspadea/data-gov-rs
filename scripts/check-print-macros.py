#!/usr/bin/env python3
"""Fail if the CLI or a library crate prints with `println!` or `eprintln!`.

Two separate defects, one matcher.

**The CLI.** `println!` panics when the write fails, so piping the CLI into a
reader that quits early - `data-gov list organizations | head -5` - killed the
process with exit 101 and a backtrace note over ordinary Unix usage (#115). The
fix routes every user-facing line through `outln!` and `errln!` in
`data-gov/tools/cli/ui/output.rs`, which recognise a closed pipe and stop
quietly.

**The library crates.** A library does not own the process it is linked into. A
line written straight to stderr from `data-gov` reaches an embedder as
unattributed noise it cannot route, level, silence or capture, and it is lost
outright to a program that collects `tracing`. Library diagnostics therefore go
through `tracing` (CLAUDE.md, "Logging discipline"); user-facing progress goes
to the embedder through `data_gov::ui::StatusReporter`. None of the three
library crates prints.

Both fixes hold only while they are complete. One `println!` added later
reopens the defect where it was added, and nothing else would notice: the code
compiles, the tests pass, and the CLI crash appears only when somebody pipes
that particular command into `head`. So the absence of the bare macros is
checked here rather than remembered.

`data-gov-mcp-server` is deliberately not scanned. It is a binary with no
`lib.rs`, nothing embeds it, and it writes its startup warnings to stderr on
purpose: stdout carries the JSON-RPC stream and may hold nothing else.

Comment lines are exempt, because `output.rs` documents the macros it replaces
by name and the client crates print in their rustdoc examples.

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

# Each scope is a path prefix, the name used in messages, and the advice a
# violation there gets - which differs, because the two defects have different
# fixes. `data-gov/src/` is the library half of the crate whose binary lives
# under `data-gov/tools/cli/`.
SCOPES: list[tuple[str, str, str]] = [
    (
        "data-gov/tools/cli/",
        "CLI",
        "Use `outln!` and `errln!` from `data-gov/tools/cli/ui/output.rs`, "
        "which stop quietly when the reader closes the pipe (#115).",
    ),
    (
        "data-gov/src/",
        "data-gov",
        "A library must not write to the embedder's terminal. Use "
        "`tracing::warn!` (or the level the message deserves) so the "
        "embedder can route it.",
    ),
    (
        "data-gov-catalog/src/",
        "data-gov-catalog",
        "A library must not write to the embedder's terminal. Use "
        "`tracing::warn!` (or the level the message deserves) so the "
        "embedder can route it.",
    ),
    (
        "data-gov-ckan/src/",
        "data-gov-ckan",
        "A library must not write to the embedder's terminal. Use "
        "`tracing::warn!` (or the level the message deserves) so the "
        "embedder can route it.",
    ),
]

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
    # What library code says instead, and the rustdoc examples that show a
    # consumer printing - which are comment lines and stay exempt.
    ('    tracing::warn!(path = %p, "could not remove it");', False),
    ('/// println!("{} datasets", results.count.unwrap_or(0));', False),
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


def tracked_sources(prefix: str) -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", f"{prefix}*.rs"],
        capture_output=True,
        text=True,
        check=True,
    )
    return [path for path in out.stdout.splitlines() if path]


def scan(prefix: str, label: str, advice: str) -> tuple[int, int]:
    """Scan one scope. Returns (files scanned, violations found).

    An empty scope is itself a violation: the code moved and this check is
    silently guarding nothing, which is the failure mode that lets the
    defect back in unnoticed.
    """
    sources = tracked_sources(prefix)
    if not sources:
        print(
            f"::error::no tracked .rs files under {prefix}. The {label} code "
            "moved, so this check is scanning nothing - point it at the new "
            "location.",
            file=sys.stderr,
        )
        return (0, 1)

    findings = 0
    for path in sources:
        with open(path, encoding="utf-8") as handle:
            for number, line in enumerate(handle, start=1):
                if is_violation(line):
                    findings += 1
                    print(f"{path}:{number}: {line.rstrip()}", file=sys.stderr)

    if findings:
        print(
            f"\n{findings} bare printing macro(s) in {label}. {advice}\n",
            file=sys.stderr,
        )
    return (len(sources), findings)


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()

    scanned = 0
    findings = 0
    for prefix, label, advice in SCOPES:
        files, bad = scan(prefix, label, advice)
        scanned += files
        findings += bad

    if findings:
        return 1

    print(f"check-print-macros: {scanned} source file(s) clean.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
