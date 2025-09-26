# data-gov MCP Server

This crate provides a [Model Context Protocol](https://modelcontextprotocol.org/) (MCP) server
exposing the high-level helpers from the `data-gov` crate and the lower-level CKAN
bindings from `data-gov-ckan`.  The server operates strictly as a thin proxy: it does
not modify either library and forwards requests directly to the underlying
implementations.

## Features

- Search data.gov datasets with the ergonomic `DataGovClient`
- Retrieve dataset details and organization metadata
- Access raw CKAN endpoints such as `package_search`, `package_show`, and
  `organization_list`
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

### High-level Data.gov tools
- `data_gov.search`: Search datasets on data.gov with optional filters
- `data_gov.dataset`: Fetch detailed metadata for a dataset by name or ID
- `data_gov.autocompleteDatasets`: Autocomplete dataset names based on a partial query
- `data_gov.listOrganizations`: List publishing organizations (agencies) on data.gov
- `data_gov.downloadResources`: Download one or more dataset resources to the local filesystem

### Low-level CKAN tools
- `ckan.packageSearch`: Perform a low-level CKAN package_search request
- `ckan.packageShow`: Retrieve detailed metadata for a dataset using CKAN
- `ckan.organizationList`: List CKAN organizations with optional sorting and pagination

### MCP protocol tools
- `tools/list`: List all available tools and their schemas
- `tools/call`: Call a tool by name with arguments
- `initialize`, `initialized`, `shutdown`: MCP protocol lifecycle

Each request is expected to follow the shape:

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

Responses mirror the JSON-RPC 2.0 schema and either contain a `result` payload or
an `error` object.


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

Environment variables allow the server to reuse API configuration without altering the upstream crates:

- `DATA_GOV_API_KEY` – CKAN API key for higher rate limits
- `DATA_GOV_BASE_URL` – Override the default CKAN base path
- `DATA_GOV_USER_AGENT` – Custom user agent applied to both clients

These settings are optional; when omitted the defaults from the existing libraries are used.

## Development

The crate lives under `tools/mcp/data-gov-mcp-server` and is part of the main
Cargo workspace.  Tests and formatting can be run from the repository root:

```bash
cargo fmt
cargo test -p data-gov-mcp-server
```
