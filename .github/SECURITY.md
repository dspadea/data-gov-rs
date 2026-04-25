# Security Policy

## Supported Versions

The project is pre-1.0; only the latest released minor receives security
fixes.

| Version | Supported          |
| ------- | ------------------ |
| 0.4.x   | :white_check_mark: |
| < 0.4   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in data-gov-rs, please **open a
GitHub issue** in this repository:

<https://github.com/dspadea/data-gov-rs/issues/new/choose>

Include:

- A description of the vulnerability
- Steps to reproduce
- Potential impact
- Any suggested fixes, if you have them

Use the same channel for non-security bug reports and feature requests —
this is a small, single-maintainer project and GitHub issues are the only
supported intake. There is no private security disclosure channel; if a
report would be harmful to publish, please describe the issue at a high
level in the issue and we'll coordinate next steps from there.

### What to expect

Best-effort response from a single maintainer. There is no formal SLA;
typical turnaround for an acknowledgment is a few days. Fixes for
confirmed issues will land in a patch release of the latest supported
minor (see the version table above), and reporters will be credited in
the changelog unless they prefer otherwise.

### Security Considerations for Government Data APIs

When working with government data APIs like data.gov and CKAN instances, please be aware of:

#### API Keys and Authentication
- **Never commit API keys** to version control
- **Use environment variables** or secure credential management
- **Rotate API keys regularly** if you have write access
- **Follow principle of least privilege** - only request the permissions you need

#### Data Sensitivity
- **Understand data classification** - even "open" data may have usage restrictions
- **Respect rate limits** - don't overload government servers
- **Handle PII carefully** - some datasets may contain personally identifiable information
- **Follow data retention policies** - don't store data longer than necessary

#### Network Security
- **Use HTTPS** - all API calls should use secure connections (this library enforces this)
- **Validate certificates** - don't disable TLS verification
- **Be aware of proxies** - corporate/government networks may intercept traffic
- **Consider data residency** - be aware of where your applications process government data

### Common Security Issues to Avoid

1. **Credential Exposure**
   ```rust
   // DON'T DO THIS - hardcoded API key
   let config = Configuration {
       api_key: Some(ApiKey {
           key: "abc123-secret-key".to_string(),
           ..Default::default()
       }),
       ..Default::default()
   };
   
   // DO THIS - use environment variables
   let api_key = std::env::var("CKAN_API_KEY").ok()
       .map(|key| ApiKey { key, prefix: None });
   let config = Configuration {
       api_key,
       ..Default::default()
   };
   ```

2. **Unvalidated Input**
   ```rust
   // Be careful with user-provided search terms
   let user_input = sanitize_search_input(&user_query);
   let results = client.package_search(Some(&user_input), None, None, None).await?;
   ```

3. **Information Disclosure**
   ```rust
   // Don't log sensitive information
   log::debug!("API response: {:?}", response); // Could contain sensitive data
   
   // Instead, log only what's needed
   log::debug!("API call successful, {} results", response.count);
   ```

### Dependencies and Supply Chain Security

- We regularly audit our dependencies using `cargo audit`
- We pin dependency versions to avoid unexpected updates
- We follow Rust security advisories and update dependencies promptly
- Consider running `cargo audit` in your own projects using this library

### Secure Development Practices

This project follows secure development practices:

- **Code review** required for all changes
- **Automated security scanning** in CI/CD
- **Dependency vulnerability monitoring**
- **Regular security updates**
- **Principle of least privilege** in API design

### Questions?

For any question about this policy, security or otherwise, open a GitHub
issue: <https://github.com/dspadea/data-gov-rs/issues/new/choose>.

---

Thank you for helping keep data-gov-rs and its users secure! 🔒