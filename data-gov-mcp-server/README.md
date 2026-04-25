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

The MCP server exposes the following tools (methods):

### Data.gov tools
- `data_gov.search` – Search datasets. Cursor-paginated via `after`, optional
  `organization` slug filter, and a client-side `organizationContains`
  substring filter. Response includes both the raw page and a compact
  `summaries` array.
- `data_gov.dataset` – Fetch full DCAT-US 3 metadata for a dataset by slug.
- `data_gov.autocompleteDatasets` – Dataset title suggestions for a partial
  query (implemented as a capped full-text search).
- `data_gov.listOrganizations` – List publishing organizations.
- `data_gov.downloadResources` – Download distributions to the local
  filesystem, optionally limited by zero-based `distributionIndexes` and/or a
  `formats` filter (matched against `format` and `mediaType`).

### MCP protocol methods
- `tools/list` – List available tools and their schemas.
- `tools/call` – Invoke a tool by name with arguments.
- `initialize`, `initialized`, `shutdown` – MCP protocol lifecycle.

Each request follows the usual JSON-RPC 2.0 shape:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "data_gov.search",
  "params": {
    "query": "climate",
    "limit": 5
  }
}
```

Responses mirror the JSON-RPC 2.0 schema and either contain a `result`
payload or an `error` object.

### Pagination

`data_gov.search` uses cursor-based pagination. When there are more pages, the
response body carries an `after` field. Pass it back unchanged on the next
call:

```jsonc
{"method": "data_gov.search", "params": {"query": "climate", "limit": 20}}
// response: { "results": [...], "after": "WzgxLjM...", ...}

{"method": "data_gov.search", "params": {"query": "climate", "limit": 20, "after": "WzgxLjM..."}}
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
