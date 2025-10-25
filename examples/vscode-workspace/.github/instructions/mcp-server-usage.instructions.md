---
applyTo: '**'
---
# MCP Server Usage Guidelines

## Primary Rule
ALWAYS use the MCP server when it provides the required functionality. Do NOT generate scripts or command lines when the MCP server can do it for you.

## Specific Requirements

### Data.gov Operations
When working with data.gov datasets:

1. **Searching**: Use `mcp_data-gov_data_gov_search` or `mcp_data-gov_ckan_package_search` instead of manual API calls
2. **Dataset Details**: Use `mcp_data-gov_data_gov_dataset` or `mcp_data-gov_ckan_package_show` instead of curl/wget to fetch metadata
3. **Downloading Files**: Use `mcp_data-gov_data_gov_download_resources` instead of wget, curl, or other download tools
   - This function provides automatic error handling, format filtering, and structured results
   - It accepts: datasetId, outputDir, formats (optional), resourceIds (optional)


## Verification
Before running any terminal command, ask yourself:
1. Does an MCP server provide this functionality?
2. If yes, use the MCP tool instead
3. If no, then proceed with the terminal command or ad-hoc code