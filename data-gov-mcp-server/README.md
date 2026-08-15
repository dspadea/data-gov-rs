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
- `ping` — Keepalive. Answers with an empty result.
- `notifications/cancelled` — Stops work on the request named by `requestId`.
  The cancelled request is never answered. A `requestId` that is unknown or
  has already finished is ignored.

Requests are dispatched concurrently, so a long download does not delay
anything sent after it. Responses may therefore arrive in a different order
from the requests; correlate them by `id`, as JSON-RPC requires. Each request
runs under a 15-minute ceiling.

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

Every message needs `"jsonrpc": "2.0"`; the member is required and a message
without it is rejected with `-32600`. A request also needs an `id` that is a
string or an integer — never `null`. A message with no `id` at all is a
notification, and notifications are never answered.

Responses mirror the JSON-RPC 2.0 schema and either contain a `result`
payload or an `error` object.

### How failures are reported

The two kinds are reported differently, and they mean different things:

- **A tool that ran and failed** returns a normal `result` with
  `"isError": true` and the reason in `content` — an upstream outage, a
  dataset with no matching downloadable distributions, a dataset with no DCAT
  metadata. This is what MCP asks for, so a model can read the reason and try
  something else. `isError` is always present, `false` on success.
- **A protocol fault** returns a JSON-RPC `error` object — an unknown method
  or tool (`-32601`), or arguments that do not match the tool's schema
  (`-32602`).

Tool arguments are validated against the advertised `inputSchema`, which
declares `additionalProperties: false` for every tool. A property the schema
does not declare is refused by name rather than dropped, so a misspelling such
as `output_dir` for `outputDir` fails loudly instead of silently running with
a different argument.

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
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"data_gov_search","arguments":{"query":"climate","limit":20}}}
// response: { "results": [...], "after": "WzgxLjM...", ... }

// Page 2 — pass the cursor back as `after`
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"data_gov_search","arguments":{"query":"climate","limit":20,"after":"WzgxLjM..."}}}
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

**The server and the CLI resolve configuration by the same rules.** Both read
`<config>/data-gov/config.toml` and the same environment variables, through one
chain. The settings table, the file's location on each platform, and the
per-setting details live in one place: the
[configuration section of the `data-gov` README](../data-gov/README.md#the-config-file).

The server takes no command-line flags, so the chain it sees is:

**environment variable > config file > built-in default**

The environment wins over the file on purpose. An MCP server is launched by a
host application with an environment of the host's choosing, and an operator
who configures the host stays in charge of it - the file supplies what the host
did not.

All five settings apply: `download_dir`, `base_url`, `max_concurrent_downloads`,
`download_timeout_secs`, and `user_agent`. `download_dir` is the default target
for `data_gov_download_resources` when a call omits `outputDir`.

Everything is optional; with nothing set anywhere the server runs on the
built-in defaults. The Catalog API does not require an API key.

A `config.toml` that cannot be read or parsed, or a value that cannot work,
stops the server at startup with a message naming the setting - it does not
run on settings the operator did not choose. Warnings go to **stderr**, never
stdout, because stdout carries the JSON-RPC stream.

## Cargo features

| Feature       | Default | Effect                                             |
|---------------|---------|-----------------------------------------------------|
| `native-tls`  | yes     | Use the platform TLS stack (`reqwest/native-tls`), forwarded through `data-gov` to `data-gov-catalog`. |
| `rustls-tls`  | no      | Use rustls instead (`reqwest/rustls`), forwarded through `data-gov` to `data-gov-catalog`.             |

To use rustls:

```toml
[dependencies]
data-gov-mcp-server = { version = "0.5", default-features = false, features = ["rustls-tls"] }
```

## Development

```bash
cargo fmt
cargo test -p data-gov-mcp-server
```


## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).
