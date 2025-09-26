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

## Usage

```bash
cargo run -p data-gov-mcp-server
```

The process reads JSON-RPC 2.0 messages (one per line) from standard input and
writes responses to standard output.  On startup it emits a `ready` message that
advertises the available methods:

- `data_gov.search`
- `data_gov.dataset`
- `data_gov.autocompleteDatasets`
- `data_gov.listOrganizations`
- `ckan.packageSearch`
- `ckan.packageShow`
- `ckan.organizationList`

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

## Configuration

Environment variables allow the server to reuse API configuration without
altering the upstream crates:

- `DATA_GOV_API_KEY` – CKAN API key for higher rate limits
- `DATA_GOV_BASE_URL` – Override the default CKAN base path
- `DATA_GOV_USER_AGENT` – Custom user agent applied to both clients

These settings are optional; when omitted the defaults from the existing
libraries are used.

## Development

The crate lives under `tools/mcp/data-gov-mcp-server` and is part of the main
Cargo workspace.  Tests and formatting can be run from the repository root:

```bash
cargo fmt
cargo test -p data-gov-mcp-server
```
