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
   - `search <query> [limit]`
   - `show <dataset-id>`
   - `download <dataset-id> [resource-index]`
   - `list organizations`
   - `info`
4. End with `quit` to cleanly exit
5. Make executable with `chmod +x script.sh`

Example:
```bash
#!/usr/bin/env data-gov
# My custom data script
search "my topic" 5
show first-dataset-id
download first-dataset-id 0
info
quit
```