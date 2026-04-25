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
2. **Dataset details**: Use `mcp_data-gov_data_gov_dataset` instead of
   curl/wget to fetch DCAT-US 3 metadata. Argument: `slug` (e.g.,
   `electric-vehicle-population-data`).
3. **Listing organizations**: Use
   `mcp_data-gov_data_gov_list_organizations`.
4. **Autocomplete**: Use `mcp_data-gov_data_gov_autocomplete_datasets` for
   dataset title suggestions.
5. **Downloading files**: Use `mcp_data-gov_data_gov_download_resources`
   instead of wget, curl, or other download tools.
   - Accepts: `datasetId` (slug), optional `outputDir`, optional `formats`
     filter (case-insensitive substring match against `format` or
     `mediaType`, so `"JSON"` matches `application/json`), and optional
     `distributionIndexes` (zero-based indexes into the downloadable
     distributions list). The legacy `resourceIds` parameter no longer
     exists — data.gov's Catalog API exposes DCAT distributions rather than
     CKAN resources.
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
