#!/usr/bin/env python3
"""A stub crates.io sparse index, for testing the release helpers offline.

The release helpers must never touch the live index from a test, so this
serves canned answers instead: the crate file, a 404, a 500, an arbitrary
status, or a run of failures followed by the real answer. It binds an
ephemeral port on the loopback interface and prints ``PORT <n>`` on stdout
before it serves, so the caller does not have to guess a free port.

Modes:
    ok            every request returns 200 and --body
    missing       every request returns 404, as the index does for a crate
                  it has never seen
    server-error  every request returns 500
    status        every request returns --status
    flaky         the first --fail-first requests return 500, then 200
    late          the first --fail-first requests return 404, then 200

Usage:
    stub-index-server.py --mode ok --body FILE
    stub-index-server.py --mode status --status 429
    stub-index-server.py --mode flaky --body FILE --fail-first 2
"""

import argparse
import http.server
import sys
import threading


def build_handler(args: argparse.Namespace, body: bytes) -> type:
    """Return a request handler that answers according to `args.mode`."""
    state = {"seen": 0}
    lock = threading.Lock()

    class Handler(http.server.BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_GET(self) -> None:  # noqa: N802 - name fixed by the base class
            with lock:
                state["seen"] += 1
                seen = state["seen"]

            early = seen <= args.fail_first
            if args.mode == "missing" or (args.mode == "late" and early):
                self.send_error(404, "Not Found")
                return
            if args.mode == "server-error" or (args.mode == "flaky" and early):
                self.send_error(500, "Internal Server Error")
                return
            if args.mode == "status":
                self.send_error(args.status, "Stub Status")
                return

            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt: str, *fmt_args: object) -> None:
            """Stay quiet; the test prints its own progress."""

    return Handler


def main() -> int:
    """Parse the arguments and serve until killed."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--mode",
        required=True,
        choices=("ok", "missing", "server-error", "status", "flaky", "late"),
    )
    parser.add_argument("--body", help="file whose contents a 200 returns")
    parser.add_argument(
        "--status",
        type=int,
        default=500,
        help="status code returned in --mode status",
    )
    parser.add_argument(
        "--fail-first",
        type=int,
        default=0,
        help="number of leading failures in --mode flaky and --mode late",
    )
    args = parser.parse_args()

    body = b""
    if args.body:
        with open(args.body, "rb") as handle:
            body = handle.read()

    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), build_handler(args, body)
    )
    print(f"PORT {server.server_address[1]}", flush=True)
    server.serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
