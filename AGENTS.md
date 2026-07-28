# data-gov-rs

## What this project is

`data-gov-rs` is Rust tooling to find, inspect, and download U.S. government
open data. It targets the data.gov [Catalog API], which replaced the retired
CKAN Action API in 2026 and serves [DCAT-US 3] metadata with cursor pagination
and no API key.

The project gives the same catalog three front doors:

- a **library**, for Rust programs that need dataset metadata or files;
- a **CLI**, for a person at a terminal;
- an **MCP server**, for an AI agent.

All three sit on one client. A defect fixed in the client is fixed for all
three, and a contract broken in the client is broken for all three.

[Catalog API]: https://resources.data.gov/catalog-api/
[DCAT-US 3]: https://resources.data.gov/resources/dcat-us/

## The crates

| Crate | Owns | Depends on |
|---|---|---|
| `data-gov-catalog` | The Catalog API wire contract: request shaping, DCAT-US 3 models, pagination | - |
| `data-gov` | Workflow on top of the catalog: downloads, configuration, the CLI binary and its REPL | `data-gov-catalog` |
| `data-gov-mcp-server` | The MCP protocol surface that exposes `data-gov` to an agent | `data-gov` |
| `data-gov-ckan` | The CKAN Action API, for portals that still run CKAN | - |

`data-gov-ckan` is not part of the data.gov path. data.gov retired its CKAN
endpoint, so this crate is a general-purpose client for other public portals -
European, state, municipal, and university instances. It is in maintenance
mode: correctness and security work lands, new features usually do not. Its
correctness bar is "works against any compliant CKAN deployment", not "works
against data.gov".

## Who the work serves

1. **A Rust program** that wants typed metadata and files, and needs the crate
   to keep its semver promises.
2. **A person at a terminal** who navigates the catalog as a filesystem:
   `/` holds organizations, an organization holds datasets, a dataset holds
   distributions. `cd`, `ls`, `pwd`, and `download` mean what a shell user
   expects them to mean.
3. **An AI agent** speaking MCP, which cannot see a screen, cannot retry on
   intuition, and believes what the server tells it.

The third consumer sets the bar for the other two. An agent acts on a tool
result without checking it, so a result that overstates what happened causes
real harm rather than confusion.

## What must stay true

These are the invariants. A change that breaks one of them is off the rails,
whatever else it improves.

- **Harvested metadata is untrusted input.** Dataset titles, formats, and
  download URLs come from third-party agency `data.json` files, not from the
  operator and not from the user. Treat every field as hostile: filenames get
  sanitized before they reach a path, URLs get checked before they reach the
  network, and no field width is assumed from one sample.

- **Downloads land inside the directory the user chose.** Nowhere else, by any
  path, including after a redirect or a sanitizer change.

- **A partial result is never reported as a whole one.** A failed download, a
  dropped field, a truncated page, and a filtered-out record each have to be
  visible in what the caller receives. Silence reads as success.

- **The zero-setup path stays zero-setup.** The keyless origin serves
  everything the tools need. Configuration and credentials are opt-in, and
  the tool works on a clean machine with no file to write first.

- **The model follows the payload, not the other way round.** A type is a claim
  about what the server sends. Change it only against a fresh capture of a real
  response, and keep the capture.

- **The MCP server answers to the published spec.** The specification decides
  what is conformant, not our own constants and not a client that happens to
  tolerate a deviation.

## Out of scope

- Writing to data.gov. The Catalog API is read-only, and so are these tools.
- A portal-agnostic abstraction over CKAN and the Catalog API. The two data
  models differ, and one merged type would serve neither well.
- A graphical interface.
- Caching or a local index of the catalog.

## Where the rest lives

- **How to work here** - the quality gate, testing rules, error handling, and
  commit and branch conventions: [CLAUDE.md](./CLAUDE.md).
- **What is outstanding** - GitHub Issues on this repository. Work in progress
  belongs in the tracker, not in a comment or a memory.
- **What changed** - [CHANGELOG.md](./CHANGELOG.md), breaking changes first.
