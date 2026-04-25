---
name: API Support Request
about: Request support for a new endpoint (Catalog API or CKAN Action API)
title: '[API] Add support for '
labels: enhancement, api
assignees: ''
---

**Which crate should this land in?**
- [ ] `data-gov-catalog` (data.gov Catalog API)
- [ ] `data-gov-ckan` (generic CKAN Action API)
- [ ] `data-gov` (high-level wrapper / CLI command)
- [ ] `data-gov-mcp-server` (new MCP tool)

**Endpoint or method**
- Path / method name: `…`
- Upstream documentation:
  - Catalog API: <https://resources.data.gov/catalog-api/>
  - CKAN: <https://docs.ckan.org/en/latest/api/>
- HTTP method: GET/POST
- Authentication required: Yes / No

**Parameters**
List the parameters this endpoint accepts:
- `param1` (string, required): Description
- `param2` (int, optional): Description

**Response format**
Describe or paste an example response:

```json
{
  "example": "data"
}
```

**Use Case**
What would you use this endpoint for? How does it fit into your workflow?

**Availability**
- [ ] Available on data.gov's Catalog API
- [ ] Available on a CKAN instance (specify which)
- [ ] Not sure

**Priority**
How important is this feature to you?
- [ ] Critical — blocking my project
- [ ] High — would significantly improve my workflow
- [ ] Medium — nice to have
- [ ] Low — just for completeness

**Implementation Notes**
Any specific considerations for implementing this endpoint?

- Special error handling needed?
- Complex response structure?
- Pagination support (cursor / offset)?
- Authentication requirements?

**Additional Context**
Any other relevant information.
