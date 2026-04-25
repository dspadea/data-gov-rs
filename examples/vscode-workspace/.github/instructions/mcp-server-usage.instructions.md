---
applyTo: '**'
---
# MCP Server Usage Guidelines

## Primary Rule
ALWAYS use the MCP server when it provides the required functionality. Do NOT
generate scripts or shell commands when an MCP tool can do it for you.

## Specific Requirements

### Data.gov Operations
When working with data.gov datasets:

1. **Searching**: Use `mcp_data-gov_data_gov_search` instead of manual API
   calls. The tool is cursor-paginated — pass the `after` value from the
   previous response to fetch the next page. An optional
   `organizationContains` substring filter is applied client-side.
2. **Dataset details**: Use `mcp_data-gov_data_gov_dataset` (lookup by slug)
   instead of curl/wget to fetch DCAT-US 3 metadata.
3. **Listing organizations**: Use
   `mcp_data-gov_data_gov_listOrganizations`.
4. **Autocomplete**: Use `mcp_data-gov_data_gov_autocompleteDatasets` for
   dataset title suggestions.
5. **Downloading files**: Use `mcp_data-gov_data_gov_downloadResources`
   instead of wget, curl, or other download tools.
   - It accepts: `datasetId` (slug), optional `outputDir`, optional
     `formats` filter (matched against both `format` and `mediaType`),
     and optional `distributionIndexes` (zero-based indexes of the
     distributions to download). The legacy `resourceIds` parameter no
     longer exists — data.gov's Catalog API exposes DCAT distributions
     rather than CKAN resources.
   - Provides automatic error handling, format filtering, and structured
     results.

### Note: legacy CKAN tools removed
Earlier versions of this MCP server exposed `mcp_data-gov_ckan_*` tools
(`packageSearch`, `packageShow`, `organizationList`). These were removed
when data.gov retired its CKAN Action API in 2026 — use the
`data_gov.*` tools listed above instead. If you need to talk to a
non-data.gov CKAN portal, depend on the `data-gov-ckan` crate directly
from your code rather than going through this MCP server.

## Verification
Before running any terminal command, ask yourself:
1. Does an MCP tool provide this functionality?
2. If yes, use the MCP tool instead.
3. If no, then proceed with the terminal command or ad-hoc code.
