# Contributing to data-gov-rs

Thank you for your interest in contributing! This workspace publishes four
Rust crates for working with US government open data:

- [`data-gov-catalog`](../data-gov-catalog/) — async client for the data.gov
  Catalog API (current backend; DCAT-US 3, cursor-paginated)
- [`data-gov`](../data-gov/) — high-level client + CLI built on
  `data-gov-catalog`
- [`data-gov-mcp-server`](../data-gov-mcp-server/) — MCP server exposing
  `data-gov` to AI tools
- [`data-gov-ckan`](../data-gov-ckan/) — generic CKAN Action API client for
  portals other than data.gov

See [`CLAUDE.md`](../CLAUDE.md) for the full development guide (testing,
error-handling, security, and dependency rules). This file is a quick
orientation for new contributors.

## Getting Started

### Prerequisites

- **Rust 1.90 or later** (the workspace uses the Rust 2024 edition)
- Git
- Basic familiarity with the data.gov [Catalog API](https://resources.data.gov/catalog-api/)
  is helpful but not required (CKAN familiarity helps for the
  `data-gov-ckan` crate specifically)

### Development Setup

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/dspadea/data-gov-rs.git
   cd data-gov-rs
   ```

3. Build the project:
   ```bash
   cargo build
   ```

4. Run tests to ensure everything works:
   ```bash
   cargo test
   ```

## How to Contribute

### Reporting Issues

- Use the GitHub issue tracker
- Search existing issues before creating new ones
- Include relevant details:
  - Rust version
  - Operating system
  - Example code that reproduces the issue
  - Error messages or unexpected behavior

### Types of Contributions Welcome

1. **Bug fixes** — Fix issues in existing functionality
2. **New endpoint coverage** — Add Catalog API endpoints in
   `data-gov-catalog`, or CKAN action endpoints in `data-gov-ckan`
3. **Documentation** — Improve docs, examples, or code comments
4. **Performance improvements** — Optimize existing code
5. **New tools / commands** — MCP tools in `data-gov-mcp-server`, REPL
   commands in `data-gov`
6. **Testing** — Add test coverage or improve existing tests

### Development Process

1. **Create a feature branch** from `main`:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. **Make your changes** following our coding standards (see below)

3. **Add tests** for new functionality:
   - Unit tests in `src/` files
   - Integration tests in `tests/` directories
   - Example usage in `examples/` directories

4. **Run the full quality gates** (CI enforces all of these):
   ```bash
   cargo fmt --all -- --check                                  # Formatting
   cargo clippy --all-targets --all-features -- -D warnings    # Lint (warnings = errors)
   cargo test --all-features                                   # Unit + fixture tests
   cargo doc --all-features --no-deps                          # Rustdoc builds clean
   ```

   Live-network integration tests are marked `#[ignore]` and run separately:
   ```bash
   cargo test -p data-gov-catalog --test integration_tests -- --ignored
   ```

5. **Update documentation** as needed:
   - Add rustdoc comments for new public APIs
   - Update README files if adding major features
   - Add or update examples

6. **Commit your changes** with a clear message:
   ```bash
   git commit -m "Add support for XYZ endpoint in CKAN client"
   ```

7. **Push to your fork** and create a pull request

## Coding Standards

### Rust Style

- Follow standard Rust naming conventions
- Use `cargo fmt` for consistent formatting
- Address all `cargo clippy` warnings
- Write clear, self-documenting code with good variable names

### Documentation

- Add rustdoc comments for all public APIs
- Include usage examples in documentation
- Update relevant README files for significant changes

### Testing

- Write unit tests for new functions
- Add integration tests for new API endpoints
- Ensure tests are reliable and don't depend on specific data that might change
- Use descriptive test names that explain what is being tested

### API Design

- Follow Rust API guidelines
- Prefer type safety over convenience where reasonable
- Use `Result` types for fallible operations
- Provide both low-level and high-level APIs when appropriate
- Consider backwards compatibility for public APIs

## Working with the underlying APIs

### Catalog API (data.gov)

- Documented at <https://resources.data.gov/catalog-api/>.
- Returns DCAT-US 3 metadata; cursor-paginated via `after`.
- Public — no API key. Be respectful with request volume; government
  servers can be slow.
- Add coverage in `data-gov-catalog` first (low-level), then surface it
  through `data-gov` (high-level) and `data-gov-mcp-server` (MCP tools)
  as needed.

### CKAN Action API (other portals)

- Documented at <https://docs.ckan.org/en/latest/api/>.
- The `data-gov-ckan` crate is no longer used by data.gov but is retained
  for European, state, municipal, and university CKAN deployments.
- Authentication via API key, basic auth, or custom headers is supported.

### Testing against live APIs

- Live-network tests are gated behind `#[ignore]`. Run them with
  `cargo test -- --ignored`.
- Use specific, stable datasets (e.g., `consumer-complaint-database`) in
  assertions; structures change less often than counts and content.
- Prefer wiremock-based fixture tests for routine coverage so CI stays
  network-independent.

### Adding new endpoints

When adding support for a new endpoint:

1. **Study the upstream documentation** (Catalog API or CKAN, as appropriate).
2. **Test manually** with `curl` or similar against a known instance.
3. **Add the response model** alongside existing types in `src/models.rs`
   (catalog) or the relevant CKAN model module.
4. **Implement the client method** in `src/client.rs`, returning the
   crate's `Result` alias.
5. **Add fixture-based tests** in `tests/` using `wiremock`.
6. **Add an `#[ignore]`'d live integration test** that hits the real API.
7. **Document the method** with a rustdoc comment, `# Errors` section, and
   a runnable example.

## Pull Request Guidelines

### Before Submitting

- Ensure all tests pass
- Update documentation
- Add appropriate examples
- Check that your branch is up to date with `main`

### PR Description

Include in your pull request:

- **What** - Clear description of what your PR does
- **Why** - Explanation of the motivation or problem being solved
- **How** - Brief overview of your approach
- **Testing** - Description of how you tested the changes
- **Breaking changes** - Note any backwards compatibility issues

### Review Process

- All PRs require review before merging
- Be responsive to feedback and questions
- Update your PR based on review comments
- Maintainers may suggest changes or alternatives

## Code of Conduct

- Be respectful and constructive in discussions
- Focus on the code and technical issues
- Help create a welcoming environment for all contributors
- Follow GitHub's community guidelines

## Questions?

- Create an issue for questions about contributing
- Check existing issues and PRs for related discussions
- Look at the project documentation and examples

Thank you for contributing to making government data more accessible through Rust! 🦀🏛️