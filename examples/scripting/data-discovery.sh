#!/usr/bin/env data-gov
# Comprehensive Data Discovery Script
# This script demonstrates various data.gov operations

# Show current configuration
info

# Search for different types of data
search climate 3
search energy 2  
search transportation 2

# List government organizations
list organizations

# Show detailed information about a specific dataset
show consumer-complaint-database

# This script doesn't download anything, but you could uncomment:
# download consumer-complaint-database 0

quit