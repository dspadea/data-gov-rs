# data-gov MCP Server

This crate provides a [Model Context Protocol](https://modelcontextprotocol.org/) (MCP)
server exposing the high-level helpers from the `data-gov` crate, which is
backed by the data.gov [Catalog API](https://resources.data.gov/catalog-api/).
The server operates as a thin proxy: it does not modify the library and
forwards requests directly to the underlying implementation.

> **2026 migration note:** data.gov retired its CKAN Action API. The server
> previously exposed low-level `ckan.*` tools alongside the high-level
> `data_gov.*` tools; the CKAN tools are gone. Use the `data_gov.*` tools below.

## Features

- Search data.gov datasets with cursor-based pagination
- Retrieve DCAT-US 3 dataset details and organization metadata
- Download DCAT distributions to the local filesystem with concurrency control
- Works over standard MCP JSON-RPC framing on STDIN/STDOUT

## Prod Readiness

This software is brand new, and has not been thoroughly tested or hardened. Use at your own risk.

## Usage

```bash
cargo run -p data-gov-mcp-server
```

The process reads JSON-RPC 2.0 messages (one per line) from standard input and writes responses to standard output. On startup it emits a `ready` message that advertises the available methods.

## Available Tools

Tools are invoked the standard MCP way: `tools/call` with the tool's `name`
and an `arguments` object. Discover them at runtime with `tools/list`.

### Data.gov tools

- `data_gov_search` — Search datasets. Cursor-paginated via `after`; optional
  `organization` slug filter and a client-side `organizationContains`
  substring filter. Response wraps the raw page plus a compact `summaries`
  array.
- `data_gov_dataset` — Fetch full DCAT-US 3 metadata for a dataset. Takes
  `slug` (e.g., `electric-vehicle-population-data`).
- `data_gov_autocomplete_datasets` — Dataset title suggestions for a partial
  query (implemented as a capped full-text search).
- `data_gov_list_organizations` — List publishing organizations.
- `data_gov_download_resources` — Download distributions to the local
  filesystem. Optional `distributionIndexes` (zero-based) and `formats`
  filter; `formats` is matched as a **case-insensitive substring** against
  each distribution's `format` and `mediaType`, so `"JSON"` matches
  `application/json`, `"CSV"` matches `text/csv`, etc.

### MCP protocol methods

- `tools/list` — List available tools and their schemas.
- `tools/call` — Invoke a tool by name with arguments.
- `initialize`, `initialized`, `shutdown` — MCP protocol lifecycle.

A typical `tools/call` request:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "data_gov_search",
    "arguments": { "query": "climate", "limit": 5 }
  }
}
```

Responses mirror the JSON-RPC 2.0 schema and either contain a `result`
payload or an `error` object.

#### Direct method dispatch (non-MCP clients)

For raw JSON-RPC clients that don't go through `tools/call`, the same tools
are also exposed under dot-camelCase method names: `data_gov.search`,
`data_gov.dataset`, `data_gov.autocompleteDatasets`,
`data_gov.listOrganizations`, `data_gov.downloadResources`. Standard MCP
clients (VSCode, Claude Desktop, etc.) only see — and only need — the
snake_case tool names above.

### Pagination

`data_gov_search` uses cursor-based pagination. When there are more pages,
the response body carries an `after` field. Pass it back unchanged on the
next call:

```jsonc
// Page 1
{"method":"tools/call","params":{"name":"data_gov_search","arguments":{"query":"climate","limit":20}}}
// response: { "results": [...], "after": "WzgxLjM...", ... }

// Page 2 — pass the cursor back as `after`
{"method":"tools/call","params":{"name":"data_gov_search","arguments":{"query":"climate","limit":20,"after":"WzgxLjM..."}}}
```

## VSCode Integration

To use the MCP server in VSCode, add the following to your workspace `.vscode/mcp.json`. Adjust the command path accordingly to the location of the MCP server.

For more information on configuring VSCode to use MCP servers:

https://code.visualstudio.com/docs/copilot/customization/mcp-servers#_add-an-mcp-server


```jsonc
{
  "servers": {
    "data-gov": {
      "type": "stdio",
      "command": "target/debug/data-gov-mcp-server",
      "args": []
    }
  },
  "inputs": []
}
```

This will configure VSCode to launch the MCP server and connect to it for tool-based workflows.

## Configuration

Environment variables:

- `DATA_GOV_BASE_URL` – Override the default Catalog API base URL
  (defaults to `https://catalog.data.gov`).
- `DATA_GOV_USER_AGENT` – Custom user agent applied to the client.

These settings are optional; when omitted the defaults from the underlying
library are used. The Catalog API does not require an API key.

## Development

```bash
cargo fmt
cargo test -p data-gov-mcp-server
```


## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).
