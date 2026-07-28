#!/bin/bash

# Demo script showing both CLI and interactive modes of data-gov

echo "🇺🇸 Data.gov CLI & REPL Demo"
echo "============================"
echo

echo "📋 1. CLI Mode Examples:"
echo "------------------------"
echo

echo "🔍 Searching for 'energy' datasets (CLI mode):"
data-gov search energy 3
echo

echo "🏛️ Listing organizations (CLI mode):"
data-gov list organizations | head -5
echo

echo "ℹ️ Showing client info (CLI mode):"
data-gov info
echo

echo "📋 2. Interactive Mode:"
echo "----------------------"
echo "Try these commands:"
echo "  search solar energy"
echo "  show consumer-complaint-database"
echo "  list organizations"
echo "  help"
echo "  quit"
echo

# Only drop into the REPL when there's a real terminal to type into. Piping
# this script (or running it from CI, cron, or another script) leaves stdin
# closed or non-interactive, and the REPL would otherwise sit waiting for
# input that never comes.
if [ -t 0 ]; then
  echo "Starting interactive REPL... (type 'quit' to exit)"
  data-gov
else
  echo "(stdin is not a terminal, so the interactive REPL is not started here.)"
  echo "Run 'data-gov' yourself to try it."
fi
