# Contributing to data-gov-rs

Thank you for your interest in contributing to data-gov-rs! This project provides Rust libraries for working with US government open data APIs, particularly data.gov and CKAN-powered portals.

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Git
- Basic familiarity with CKAN APIs and data.gov (helpful but not required)

### Development Setup

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/your-username/data-gov-rs.git
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

1. **Bug fixes** - Fix issues in existing functionality
2. **New API endpoints** - Add support for additional CKAN API endpoints
3. **Documentation** - Improve docs, examples, or code comments  
4. **Performance improvements** - Optimize existing code
5. **New data sources** - Add support for other government APIs
6. **Testing** - Add test coverage or improve existing tests

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

4. **Run the full test suite**:
   ```bash
   # Run all tests
   cargo test
   
   # Run integration tests (requires network)
   cd data-gov-ckan && cargo test --test integration_tests
   
   # Check formatting
   cargo fmt --check
   
   # Run clippy
   cargo clippy -- -D warnings
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

## Working with CKAN APIs

### Understanding the Data

- CKAN is used by data.gov and many other government data portals
- APIs follow REST principles with JSON responses
- Most endpoints support both GET and POST methods
- Authentication via API keys is optional but recommended for higher rate limits

### Testing Against Real APIs

- Integration tests run against the real data.gov API
- Be mindful of rate limits during development
- Some tests may be flaky due to network conditions - that's expected
- Use specific, stable datasets in tests when possible

### Adding New Endpoints

When adding support for new CKAN API endpoints:

1. **Study the CKAN documentation** for the endpoint
2. **Test manually** with curl or similar tools
3. **Add appropriate models** in the `models/` directory
4. **Implement the client method** in `client.rs`
5. **Add comprehensive tests** including error cases
6. **Document with examples** showing real usage

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