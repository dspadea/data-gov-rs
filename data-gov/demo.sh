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
echo "Starting interactive REPL... (type 'quit' to exit)"
echo "Try these commands:"
echo "  search solar energy"
echo "  show consumer-complaint-database"  
echo "  list organizations"
echo "  help"
echo "  quit"
echo

# Start interactive mode
data-gov