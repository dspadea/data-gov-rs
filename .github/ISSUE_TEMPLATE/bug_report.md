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
use data_gov_ckan::CkanClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CkanClient::new_data_gov(None)?;
    // ... rest of your code
    Ok(())
}
```

**Environment (please complete the following information):**
- OS: [e.g. Ubuntu 22.04, macOS 13.0, Windows 11]
- Rust version: [e.g. 1.70.0] (run `rustc --version`)
- Library version: [e.g. 0.1.0]
- CKAN instance: [e.g. data.gov, demo.ckan.org, other]

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