# Security Policy

## Supported Versions

We actively support the following versions of data-gov-rs:

| Version | Supported          |
| ------- | ------------------ |
| 3.x.x   | :white_check_mark: |
| 2.x.x   | :x:                |
| < 2.0   | :x:                |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security vulnerability in data-gov-rs, please follow these steps:

### For Security Issues

**DO NOT** create a public GitHub issue for security vulnerabilities.

Instead, please:

1. **Email us directly** at [security@yourproject.com] with details about the vulnerability
2. **Include the following information:**
   - Description of the vulnerability
   - Steps to reproduce the issue
   - Potential impact
   - Any suggested fixes (if you have them)
   - Your contact information

3. **Wait for our response** - We will acknowledge your report within 48 hours and provide a detailed response within 7 days.

### What to Expect

- **Acknowledgment**: We'll confirm receipt of your vulnerability report
- **Assessment**: We'll assess the severity and impact of the vulnerability  
- **Fix Development**: We'll work on a fix and coordinate the release timeline with you
- **Disclosure**: We'll coordinate responsible disclosure of the vulnerability
- **Credit**: We'll credit you appropriately (if desired) when we announce the fix

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

If you have questions about security practices or this policy, please create a public issue (for general questions) or contact us privately (for security-sensitive topics).

---

Thank you for helping keep data-gov-rs and its users secure! 🔒