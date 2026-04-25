# Data.gov Script Examples

This directory contains example scripts that demonstrate how to use `data-gov` with shebang (`#!/usr/bin/env data-gov`) for automation.

## Scripts

- **`climate-search.sh`** - Basic search and info display
- **`download-epa-climate.sh`** - Search and download EPA climate data
- **`list-orgs.sh`** - List government organizations  
- **`auto-download.sh`** - Automated dataset download
- **`data-discovery.sh`** - Comprehensive data exploration

## Usage

All scripts can be run directly after making them executable:

```bash
chmod +x script-name.sh
./script-name.sh
```

Or you can pipe them to the `data-gov` binary:

```bash
data-gov < script-name.sh
```

## Script Features

- **Comments**: Lines starting with `#` are ignored
- **Shebang Support**: `#!/usr/bin/env data-gov` works when `data-gov` is in PATH
- **REPL Mode**: Scripts run in interactive mode (downloads go to `~/Downloads/<dataset>/`)
- **Sequential Execution**: Commands run one after another
- **Error Tolerance**: Script continues even if individual commands fail

## Creating Your Own Scripts

1. Start with the shebang line: `#!/usr/bin/env data-gov`
2. Add comments to describe what the script does
3. Use any combination of data-gov commands:
   - `cd <path>` — navigate (`/`, `/<org>`, `/<org>/<dataset>`, or just `/<dataset>`; validated against the catalog)
   - `ls` — list contents of the current location
   - `next` (or `n`) — fetch the next page of the most recent `search` or `ls`
   - `search <query> [limit]` — full-text search (filtered by active org if any)
   - `show [dataset_slug|.]` — show dataset details (`.` = current)
   - `download [dataset_slug] [selectors...]` — download by zero-based index or title substring
   - `list organizations` — bulk org list (regardless of context)
   - `info`, `help`, `quit`
4. End with `quit` to cleanly exit
5. Make executable with `chmod +x script.sh`

The REPL treats the data.gov catalog as a four-level Unix-style
filesystem: root → organizations → datasets → distributions. `cd` and
`ls` work the way you'd expect from a shell, and `cd` validates each
path against the catalog before adopting it, so a typo errors loudly
instead of leaving you in a bogus location.

Example using the unix-fs idiom:

```bash
#!/usr/bin/env data-gov
# Navigate into a dataset, list its distributions, download the first one.
cd /electric-vehicle-population-data
ls
download 0
quit
```

Example using the legacy one-shot idiom (still supported):

```bash
#!/usr/bin/env data-gov
# Search and download in flat command form.
search "my topic" 5
show first-dataset-id
download first-dataset-id 0
info
quit
```