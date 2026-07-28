# data-gov

High-level Rust client and CLI for [data.gov](https://data.gov). It wraps the low-level [`data-gov-catalog`](../data-gov-catalog/) crate with download helpers, an interactive REPL, and ergonomic configuration.

> **2026 migration note:** data.gov retired its CKAN Action API. This crate now
> targets the purpose-built
> [Catalog API](https://resources.data.gov/catalog-api/) via `data-gov-catalog`.
> The Catalog API uses cursor-based pagination, returns DCAT-US 3 metadata, and
> is publicly accessible (no API key).

## Requirements

- Rust **1.90+** (Rust 2024 edition)
- Cargo and git

```bash
rustup toolchain install stable
rustup default stable
```

## Add to your project

Use the published crate from crates.io:

```toml
[dependencies]
data-gov = "0.5"
tokio = { version = "1", features = ["full"] }
```

Working inside this repository? You can still use a path dependency in `Cargo.toml`:

```toml
data-gov = { path = "../data-gov" }
```

Need unreleased features between tags? Swap in the git dependency form instead:

```toml
data-gov = { git = "https://github.com/dspadea/data-gov-rs", package = "data-gov" }
```

### CLI install

```bash
git clone https://github.com/dspadea/data-gov-rs.git
cd data-gov-rs/data-gov
cargo install --path .
```

The `data-gov` binary is then available on your PATH.

## Highlights

- 🔍 Search data.gov with optional organization filter
- 📦 Retrieve DCAT-US 3 dataset metadata and enumerate downloadable distributions
- ⬇️ Download individual distributions or entire datasets with progress bars
- 🏛️ List organisations and suggest dataset titles
- 🖥️ Interactive REPL with colour-aware output and shebang-friendly scripts

## Library quick start

```rust
use data_gov::DataGovClient;

#[tokio::main]
async fn main() -> data_gov::Result<()> {
    let client = DataGovClient::new()?;

    let page = client.search("climate change", Some(10), None, None).await?;
    println!("Found {} results on this page", page.results.len());

    let hit = client.get_dataset("consumer-complaint-database").await?;
    println!("Dataset: {}", hit.title.as_deref().unwrap_or(""));

    if let Some(dcat) = hit.dcat.as_ref() {
        let distributions = DataGovClient::get_downloadable_distributions(dcat);
        if let Some(distribution) = distributions.first() {
            let path = client.download_distribution(distribution, None).await?;
            println!("Downloaded to {path:?}");
        }
    }

    Ok(())
}
```

## CLI overview

The REPL treats the data.gov catalog as a four-level Unix-style filesystem:

```
/                           → root (organizations live here)
/<org>/                     → an organization's datasets
/<org>/<dataset>/           → a dataset's downloadable distributions
```

`cd` and `ls` work the way you'd expect from a shell. Every `cd` is
validated against the catalog before adopting the new context, so a
typo doesn't silently leave you in a bogus location.

### REPL session walkthrough

```
$ data-gov
🇺🇸 Data.gov Interactive Explorer

data-gov:/> ls                              # list organizations
 1. census
 2. noaa
 3. epa
 ...
data-gov:/> cd /epa                         # validated against the org list
OK Active context: /epa
data-gov:/epa> ls                           # datasets in EPA, paginated 50 at a time
ambient-air-quality-data-inventory  Ambient Air Quality Data Inventory  [modified 2025-07-31]
xrd-raw-data  XRD Raw data  [1 file, modified 2026-04-21]
...
Found 50 datasets (type 'next' for more)
data-gov:/epa> next                         # advance one page
... 50 more datasets ...
data-gov:/epa> cd integrated-risk-information-system-iris
OK Active context: /epa/integrated-risk-information-system-iris
data-gov:/epa/integrated-risk-information-system-iris> ls    # distributions
 0. (untitled) [text/csv]
 1. (untitled) [application/json]
data-gov:/epa/integrated-risk-information-system-iris> show .   # '.' = current dataset
... dataset details ...
data-gov:/epa/integrated-risk-information-system-iris> download 0
... downloads distribution[0] ...
data-gov:/epa/integrated-risk-information-system-iris> cd ..
OK Active context: /epa
```

Notes on the metaphor:

- `cd /<single-segment>` resolves as either an org *or* a dataset slug —
  data.gov has a flat slug namespace, so the REPL tries org first and
  falls back to dataset. When a single segment matches a dataset, the
  org context is auto-populated from the dataset's publisher.
- `cd ..` walks up one level. `cd /` returns to root.
- `.` always means "the current dataset" in commands that take a slug
  (e.g. `show .`, where supported). Errors clearly when nothing is
  selected.
- Distribution indexes in `ls` are zero-based and match what `download N`
  expects — no off-by-one between displayed and addressable indexes.
- `next` advances the most recent paginated `search` or `ls`. `cd` clears
  the cursor (so a stale `next` doesn't reach back into the previous
  location).

### One-shot CLI usage

The same commands work as one-shot invocations from your shell:

```
data-gov search "climate change" 5
data-gov show electric-vehicle-population-data
data-gov download electric-vehicle-population-data 0                                 # by index
data-gov download electric-vehicle-population-data "Comma Separated Values File"    # by title (quoted)
data-gov download electric-vehicle-population-data csv                               # partial title match
data-gov ls                                                                          # at root, lists orgs
```

Key defaults, used when nothing sets a download directory:

- **Interactive mode:** `data-gov` launches the REPL and stores downloads under `~/Downloads/<dataset>/`
- **Non-interactive mode:** Commands run directly in your current directory (`./<dataset>/`)
- Override download location with `--download-dir` (honoured in both modes), toggle colours with `--color`, and silence progress bars via `NO_PROGRESS=1`

Settings can also be persisted in a config file instead of passed every time — see [Configuration](#configuration).

### Command reference

| Command | Purpose |
| ------- | ------- |
| `cd <path>` | Navigate to an org or dataset (validated). Examples: `cd /epa`, `cd /epa/air-quality-data`, `cd ..`, `cd /` |
| `ls` | List the contents of the current location (orgs at `/`, datasets at `/<org>`, distributions at `/<org>/<dataset>`). Paginated 50 at a time |
| `next` (alias `n`) | Fetch the next page of the most recent `ls` or `search` |
| `search <query> [limit]` | Full-text search; honors active org filter; results paginate via `next` |
| `show [dataset_slug\|.]` | Show dataset info; `.` or omitted means the current dataset |
| `download [dataset_slug] [selectors...]` | Download distributions by zero-based index or title substring; with no selectors, downloads all |
| `list organizations` | Bulk org list (regardless of context) |
| `lcd <path>` | Change the active download directory (REPL only) |
| `info` | Display current session and client configuration |
| `help`, `quit` | Help and exit commands |

### Automation

Two ways to run a sequence of commands without typing them interactively.

**Shebang scripts.** Start the script with `#!/usr/bin/env data-gov` and make
it executable. A kernel shebang launch passes the script's own path as
`data-gov`'s first argument; `data-gov` checks that argument against its
known command names first (so `search` never gets mistaken for a filename),
and when it isn't one, runs the file as a script instead - one command per
line, blank lines and `#` comments skipped (which is why the shebang line
itself is safely ignored), `quit` stops the script early.

```bash
#!/usr/bin/env data-gov
# Simple automation example
cd /epa
ls
search "electric vehicle" 3
cd /electric-vehicle-population-data
show .
download 0
quit
```

```bash
chmod +x myscript.sh
./myscript.sh
```

**Redirected stdin.** The interactive REPL reads its commands from stdin, so
piping a script in works too, with no shebang line needed:

```bash
data-gov < myscript.sh
```

Either way, a line that fails to parse or execute is reported against its
line number and the script keeps going - but the process still exits
non-zero if any line failed, so a `set -e` caller sees the failure.

`lcd` is not available in a script (same restriction as any other one-shot
`data-gov` invocation): pass `--download-dir` on the invocation instead.

See [`../examples/scripting`](../examples/scripting) for ready-made scripts such as `download-epa-climate.sh` and `list-orgs.sh`.

### Pagination

In the REPL, `search` and `ls` results paginate automatically — type `next`
(or `n`) to advance, and the previous-page cursor is forgotten when you `cd`.
Programmatically, the underlying client uses cursor-based pagination:

```rust
let page1 = client.search("climate", Some(20), None, None).await?;
let page2 = client
    .search("climate", Some(20), page1.after.as_deref(), None)
    .await?;
```

There is no random-access offset — pages can only be walked forward in order.

### Advanced filters

Use [`data_gov::catalog::CatalogClient`](https://docs.rs/data-gov-catalog) and
[`SearchParams`](https://docs.rs/data-gov-catalog) directly for keyword,
spatial, or organization-type filters not exposed on the high-level `search`.

## Configuration

### In code

```rust
use data_gov::{DataGovClient, DataGovConfig, OperatingMode};

let config = DataGovConfig::new()
    .with_mode(OperatingMode::CommandLine)
    .with_download_dir("./data")
    .with_max_concurrent_downloads(5);

let client = DataGovClient::with_config(config)?;
```

A download directory set this way is honoured in **both** operating modes. The
mode decides only what happens when no directory was chosen: the system
Downloads folder in `Interactive`, the process working directory in
`CommandLine`.

### The config file

`data-gov` reads `<config>/data-gov/config.toml`. The file is optional — an
absent one means "all defaults" and is not an error.

| Platform | Location |
| -------- | -------- |
| Linux | `$XDG_CONFIG_HOME/data-gov/config.toml`, else `~/.config/data-gov/config.toml` |
| macOS | `~/Library/Application Support/data-gov/config.toml` |
| Windows | `%APPDATA%\data-gov\config.toml` |

`XDG_CONFIG_HOME` relocates the directory on **every** platform when it is set
to an absolute path, so one exported variable isolates a container, a CI job,
or a second profile without a platform check. A relative value is ignored, as
the XDG base directory specification requires.

```toml
# <config>/data-gov/config.toml
download_dir = "/mnt/data/datagov"
base_url = "https://catalog.data.gov"
max_concurrent_downloads = 5
download_timeout_secs = 300
user_agent = "my-tool/1.0"
```

A key this build does not recognise prints a warning and is ignored, so a file
written by a newer release still works with an older binary. Invalid TOML, or a
value that cannot work (a zero concurrency limit, a `base_url` that is not
`http` or `https`), is a hard error naming the setting and where the value came
from.

**No secret belongs in this file.** An API key gets its own `0600` file.

### Precedence

Every setting resolves through one chain, highest first:

**command-line flag > environment variable > config file > built-in default**

| Config key | Environment variable | Default |
| ---------- | -------------------- | ------- |
| `download_dir` | `DATA_GOV_DOWNLOAD_DIR` | `~/Downloads` in the REPL, the working directory otherwise |
| `base_url` | `DATA_GOV_BASE_URL` | `https://catalog.data.gov` |
| `max_concurrent_downloads` | `DATA_GOV_MAX_CONCURRENT_DOWNLOADS` | `3` |
| `download_timeout_secs` | `DATA_GOV_DOWNLOAD_TIMEOUT_SECS` | `300` |
| `user_agent` | `DATA_GOV_USER_AGENT` | `data-gov-rs/<version>` |

An empty environment variable (`DATA_GOV_DOWNLOAD_DIR=`) reads as unset, not as
an empty value.

Library consumers can drive the same chain, and see which layer supplied each
value:

```rust
use data_gov::config::{ConfigOverrides, ConfigResolver, SettingKey};
use data_gov::OperatingMode;

let resolved = ConfigResolver::from_process()?
    .with_flags(ConfigOverrides::default().with_download_dir("/mnt/data"))
    .with_mode(OperatingMode::CommandLine)
    .resolve()?;

for warning in resolved.warnings() {
    eprintln!("Warning: {warning}");
}
println!("{}", resolved.setting(SettingKey::DownloadDir)); // download_dir = /mnt/data (flag)

let config = resolved.into_config();
```

`ConfigResolver::new()` reads nothing it was not given — no environment, no
file, no home directory — which is what makes the chain testable without
mutating process state.

## Cargo features

| Feature       | Default | Effect                                             |
|---------------|---------|-----------------------------------------------------|
| `native-tls`  | yes     | Use the platform TLS stack (`reqwest/native-tls`), forwarded to `data-gov-catalog`. |
| `rustls-tls`  | no      | Use rustls instead (`reqwest/rustls`), forwarded to `data-gov-catalog`.             |

To use rustls:

```toml
[dependencies]
data-gov = { version = "0.5", default-features = false, features = ["rustls-tls"] }
```

## Development

```bash
cd data-gov-rs
cargo test -p data-gov
cargo run -p data-gov --example demo
```

The crate re-exports `data-gov-catalog` as `data_gov::catalog`, making the lower-level client available when you need direct Catalog API access.

## Contributing & license

- Fork, branch, add tests, run `cargo test`, open a PR
- Licensed under [Apache 2.0](../LICENSE)


## Disclaimer & license

This is an independent project and is not affiliated with data.gov or any government agency. For authoritative information, refer to the official [data.gov](https://www.data.gov/) portal.

Licensed under the [Apache License 2.0](LICENSE).
