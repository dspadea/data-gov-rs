---
name: Bug Report
about: Create a report to help us improve the library
title: '[BUG] '
labels: bug
assignees: ''
---

**Describe the bug**
A clear and concise description of what the bug is.

**To Reproduce**
Steps to reproduce the behavior:
1. Create a client with configuration '...'
2. Call method '....'
3. Pass parameters '....'
4. See error

**Expected behavior**
A clear and concise description of what you expected to happen.

**Actual behavior**
What actually happened, including any error messages.

**Code example**
```rust
// Minimal code example that reproduces the issue
use data_gov::DataGovClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = DataGovClient::new()?;
    // ... rest of your code
    Ok(())
}
```

**Environment (please complete the following information):**
- OS: [e.g. Ubuntu 22.04, macOS 13.0, Windows 11]
- Rust version: [e.g. 1.90.0] (run `rustc --version`)
- Crate(s) and version: [e.g. data-gov 0.4.0, data-gov-mcp-server 0.4.0]
- API target: [e.g. data.gov Catalog API, demo.ckan.org, other CKAN instance]

**Error output**
If applicable, paste the complete error message:
```
Error message here
```

**Network details (if relevant)**
- Are you behind a corporate firewall?
- Any proxy settings?
- Does the same request work with curl?

**Additional context**
Add any other context about the problem here. For example:
- Does this happen consistently or intermittently?
- Did this work in a previous version?
- Any relevant network conditions or API changes?