---
name: API Support Request
about: Request support for a new CKAN API endpoint
title: '[API] Add support for '
labels: enhancement, api
assignees: ''
---

**CKAN API Endpoint**
Which CKAN API endpoint would you like to see supported?

- Endpoint path: `/api/3/action/[endpoint_name]`
- CKAN documentation: [link to docs]
- Method: GET/POST
- Authentication required: Yes/No

**API Details**

**Parameters:**
List the parameters this endpoint accepts:
- `param1` (string, required): Description
- `param2` (int, optional): Description

**Response format:**
Describe or paste an example response:

```json
{
  "success": true,
  "result": {
    "example": "data"
  }
}
```

**Use Case**
What would you use this endpoint for? How does it fit into your workflow?

**Data.gov Support**
- [ ] This endpoint is available on data.gov
- [ ] This endpoint is not available on data.gov
- [ ] I'm not sure

**Priority**
How important is this feature to you?
- [ ] Critical - blocking my project
- [ ] High - would significantly improve my workflow  
- [ ] Medium - nice to have
- [ ] Low - just for completeness

**Implementation Notes**
Any specific considerations for implementing this endpoint?

- Special error handling needed?
- Complex response structure?
- Pagination support?
- Authentication requirements?

**Additional Context**
Any other relevant information about this API endpoint.