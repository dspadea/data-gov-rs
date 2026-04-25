---
name: Feature Request
about: Suggest an idea for this project
title: '[FEATURE] '
labels: enhancement
assignees: ''
---

**Is your feature request related to a problem? Please describe.**
A clear and concise description of what the problem is. Ex. I'm always frustrated when [...]

**Describe the solution you'd like**
A clear and concise description of what you want to happen.

**Describe alternatives you've considered**
A clear and concise description of any alternative solutions or features you've considered.

**Which crate should this land in?**
- [ ] `data-gov-catalog` (data.gov Catalog API)
- [ ] `data-gov-ckan` (generic CKAN Action API)
- [ ] `data-gov` (high-level client / CLI)
- [ ] `data-gov-mcp-server` (MCP tool)

**Underlying-API context (if applicable)**
- Which endpoint(s) would this use?
- Link to upstream docs (Catalog API or CKAN):
- Example response (if you have one):

```json
{
  "example": "response"
}
```

**Use case**
Describe your specific use case - what are you trying to accomplish? This helps us understand the motivation and design the best API.

**Proposed API (optional)**
If you have thoughts on how this should look in Rust:

```rust
// Example of how you'd like to use this feature
let result = client.new_method(params).await?;
```

**Additional context**
Add any other context or screenshots about the feature request here.

**Would you be interested in implementing this?**
- [ ] Yes, I'd like to work on this
- [ ] Maybe, with some guidance
- [ ] No, but I'd be happy to test it