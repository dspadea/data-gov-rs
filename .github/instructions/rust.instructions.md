---
applyTo: '*.rs'
---

1. **Project Structure**: Understand the overall structure of the project, including key modules and their responsibilities.

2. **Coding Style**: Follow the established coding style and conventions used throughout the project. This includes naming conventions, code organization, and documentation practices.

3. **Error Handling**: Pay attention to error handling patterns used in the project. Use the appropriate error types and handling strategies.

4. **Testing**: Write tests for any new functionality or changes made to existing code. Follow the project's testing framework and conventions. Always make sure tests pass, and consider adding new tests to cover edge cases. When fixing failing tests, ensure that the root cause is addressed. Don't change the test unless the test is actually incorrect. 

5. **Performance**: Consider the performance implications of any code changes. Optimize for efficiency where possible, without sacrificing readability.

6. **Security**: Be mindful of security best practices, especially when handling user input or sensitive data.

7. **Documentation**: Update documentation to reflect any changes made to the codebase. This includes inline comments, module documentation, and external documentation.

8. **Collaboration**: Communicate effectively with other team members. Seek feedback and collaborate on code reviews to ensure high-quality contributions.

9. **Maintainability**: Always run `cargo fmt` and `cargo clippy` to ensure code quality and consistency before committing changes. Address clippy warnings unless there is a justified reason not to. Run clippy with `cargo clippy --all-targets --all-features -- -D warnings` to treat warnings as errors.

10. **Dependencies**: Be cautious when adding new dependencies. Ensure they are necessary, well-maintained, and compatible with the project's license.

11. **Version Control**: Use meaningful commit messages that clearly describe the changes made. Follow the project's branching and merging strategies.