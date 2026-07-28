#!/usr/bin/env python3
"""Fail if the tracked tree contains an absolute path to somebody's home directory.

A committed `/home/<name>` path works only for the person who wrote it. Everyone
else gets a file-not-found, and the repository has leaked a username.

The hard part is not finding the paths. It is not crying wolf: a URL such as
`https://example.com/home/webmap` and a documentation placeholder such as
`/home/user` both look exactly like the thing we are hunting for. So this scans
for the path shape, then drops two classes of match:

  1. the match sits inside a URL, and
  2. the user name is a generic placeholder rather than a person.

Run with --self-test to check the matcher itself against the cases below. The
gate runs that first, so a change that breaks the matcher fails immediately
rather than silently passing everything.
"""

from __future__ import annotations

import re
import subprocess
import sys

# `/mnt/c/Users/alice` is the ordinary shape of a Windows home directory seen
# from WSL, so the root must be recognised anywhere in a path, not only at its
# start. Windows drive letters are case-insensitive and are not always C.
PATH_RE = re.compile(
    r"""(?P<root>
          /home/
        | /Users/
        | [A-Za-z]:[\\/]{1,2}Users[\\/]{1,2}
        )
        (?P<name>(?=[A-Za-z0-9._-]*[A-Za-z0-9])[A-Za-z0-9._-]+)
    """,
    re.VERBOSE | re.IGNORECASE,
)

# Names that identify no one. A path built from these is documentation or a
# platform convention, not a leak. `runner` is the GitHub Actions home;
# `ubuntu`, `vscode`, and `node` are container conventions.
GENERIC_NAMES = frozenset(
    {
        "administrator",
        "all users",
        "default",
        "defaultuser",
        "docker",
        "linuxbrew",
        "me",
        "name",
        "node",
        "public",
        "root",
        "runner",
        "runneradmin",
        "shared",
        "someone",
        "ubuntu",
        "user",
        "username",
        "vscode",
        "you",
        "youruser",
    }
)

# This file necessarily contains the patterns and the test cases below, so it
# would report itself. Nothing else is exempt.
SELF = "scripts/check-home-paths.py"

# Token delimiters, used to isolate the word a match sits inside so a URL can be
# recognised.
DELIMITERS = ' \t"\'`(),=[]{}<>|'


def is_inside_url(line: str, start: int) -> bool:
    """True if the match at `start` is part of a URL rather than a filesystem path."""
    token_start = max((line.rfind(d, 0, start) for d in DELIMITERS), default=-1)
    return "://" in line[token_start + 1 : start]


def violations_in(line: str) -> list[str]:
    """Every home path in `line` that names a real person."""
    found = []
    for match in PATH_RE.finditer(line):
        if match.group("name").lower() in GENERIC_NAMES:
            continue
        if is_inside_url(line, match.start()):
            continue
        found.append(match.group(0))
    return found


def tracked_files() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], capture_output=True, text=True, check=True
    ).stdout
    return [p for p in out.split("\0") if p and p != SELF]


def scan() -> int:
    failures = 0
    for path in tracked_files():
        try:
            with open(path, encoding="utf-8") as handle:
                for number, line in enumerate(handle, start=1):
                    for hit in violations_in(line):
                        print(f"{path}:{number}: {hit}")
                        failures += 1
        except (UnicodeDecodeError, OSError):
            continue  # binary or unreadable; nothing to match in it
    if failures:
        print(
            f"\n{failures} absolute home path(s) above. Use $HOME, a "
            "workspace-relative path, or a bracketed placeholder such as "
            "/home/<user>.",
            file=sys.stderr,
        )
        return 1
    return 0


MUST_CATCH = [
    "/home/jsmith/.cargo/bin/data-gov-mcp-server",
    '"command": "/mnt/c/Users/jsmith/.cargo/bin/data-gov-mcp-server"',
    r"set PATH=c:\users\dave\.cargo\bin;%PATH%",
    r'"D:\Users\dave\.cargo\bin\tool.exe"',
    r'"command": "C:\\Users\\dave\\.cargo\\bin\\tool.exe"',
    "/home/al/.cargo/bin",
    "/Users/jsmith/Library/Caches",
]

MUST_IGNORE = [
    "https://services.arcgis.com/home/webmap/viewer.html",
    "see /home/<user>/.config for the path",
    "install it to /home/user/.cargo/bin",
    "copy the binary to /home/username/bin",
    "GITHUB_WORKSPACE defaults under /home/runner/work",
    "$HOME/.config/data-gov/api-key",
    "a path segment inside a URL - arcgis.com/home/... - is not a home directory",
    r'let windows = "C:\Users\me\\..\\..\\evil.txt";',
    "the home page lists every dataset",
]


def self_test() -> int:
    failures = 0
    for case in MUST_CATCH:
        if not violations_in(case):
            print(f"self-test: should have been caught but was not: {case}")
            failures += 1
    for case in MUST_IGNORE:
        hits = violations_in(case)
        if hits:
            print(f"self-test: should have been ignored but matched {hits}: {case}")
            failures += 1
    if failures:
        print(f"\n{failures} self-test failure(s)", file=sys.stderr)
        return 1
    print(f"self-test: {len(MUST_CATCH)} caught, {len(MUST_IGNORE)} ignored, as expected")
    return 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else scan())
