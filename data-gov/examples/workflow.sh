#!/bin/bash

# Practical workflow example: Find and download EPA climate data

echo "🌍 Practical Example: Finding EPA Climate Data"
echo "=============================================="
echo

echo "Step 1: Search for EPA climate datasets"
data-gov search "epa climate" 5
echo

echo "Step 2: Get detailed info about a specific dataset"
echo "(Note: Using a known stable dataset for demo)"
data-gov show climate-change-indicators-in-the-united-states
echo

echo "Step 3: Check what's in the downloads directory"
ls -la ./downloads/ 2>/dev/null || echo "Downloads directory not yet created"
echo

echo "🎯 Try downloading a resource:"
echo "data-gov download climate-change-indicators-in-the-united-states 0"
echo

echo "✨ Interactive exploration:"
echo "data-gov"