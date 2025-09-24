use std::io::{stdout, stderr};
use std::env;
use colored::{Colorize, ColoredString};
use is_terminal::IsTerminal;

/// Color mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,   // Use TTY detection
    Always, // Always use colors
    Never,  // Never use colors
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Auto
    }
}

impl std::str::FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" => Ok(ColorMode::Always),
            "never" => Ok(ColorMode::Never),
            _ => Err(format!("Invalid color mode: '{}'. Valid options: auto, always, never", s)),
        }
    }
}

/// TTY-aware color helper that respects NO_COLOR and terminal detection
#[derive(Clone)]
pub struct ColorHelper {
    mode: ColorMode,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
    no_color: bool,
}

impl ColorHelper {
    /// Create a new color helper with the specified mode
    pub fn new(mode: ColorMode) -> Self {
        Self {
            mode,
            stdout_is_terminal: stdout().is_terminal(),
            stderr_is_terminal: stderr().is_terminal(),
            no_color: env::var("NO_COLOR").is_ok() && !env::var("NO_COLOR").unwrap_or_default().is_empty(),
        }
    }

    /// Check if colors should be used for stdout
    pub fn should_color_stdout(&self) -> bool {
        self.should_use_colors(self.stdout_is_terminal)
    }

    /// Check if colors should be used for stderr
    pub fn should_color_stderr(&self) -> bool {
        self.should_use_colors(self.stderr_is_terminal)
    }

    /// Internal logic for color determination
    fn should_use_colors(&self, is_terminal: bool) -> bool {
        // Respect NO_COLOR environment variable (standard)
        if self.no_color {
            return false;
        }

        match self.mode {
            ColorMode::Never => false,
            ColorMode::Always => true,
            ColorMode::Auto => is_terminal,
        }
    }

    /// Apply red color if colors are enabled
    pub fn red(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.red()
        } else {
            text.normal()
        }
    }

    /// Apply green color if colors are enabled
    pub fn green(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.green()
        } else {
            text.normal()
        }
    }

    /// Apply blue color if colors are enabled
    pub fn blue(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.blue()
        } else {
            text.normal()
        }
    }

    /// Apply yellow color if colors are enabled
    pub fn yellow(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.yellow()
        } else {
            text.normal()
        }
    }

    /// Apply cyan color if colors are enabled
    pub fn cyan(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.cyan()
        } else {
            text.normal()
        }
    }

    /// Apply magenta color if colors are enabled
    pub fn magenta(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.magenta()
        } else {
            text.normal()
        }
    }

    /// Apply bold formatting if colors are enabled
    pub fn bold(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.bold()
        } else {
            text.normal()
        }
    }

    /// Apply dimmed formatting if colors are enabled
    pub fn dimmed(&self, text: &str) -> ColoredString {
        if self.should_color_stdout() {
            text.dimmed()
        } else {
            text.normal()
        }
    }

    /// Chainable color and formatting methods
    pub fn style(&self) -> StyleBuilder {
        StyleBuilder::new(self.should_color_stdout())
    }
}

/// Builder for chaining color and formatting operations
pub struct StyleBuilder {
    should_color: bool,
}

impl StyleBuilder {
    pub fn new(should_color: bool) -> Self {
        Self { should_color }
    }

    pub fn red(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).red()
    }

    pub fn green(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).green()
    }

    pub fn blue(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).blue()
    }

    pub fn yellow(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).yellow()
    }

    pub fn cyan(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).cyan()
    }

    pub fn magenta(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).magenta()
    }

    pub fn bold(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).bold()
    }

    pub fn dimmed(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color).dimmed()
    }
}

/// Chainable style operations
pub struct ChainedStyle {
    text: String,
    should_color: bool,
}

impl ChainedStyle {
    fn new(text: &str, should_color: bool) -> Self {
        Self {
            text: text.to_string(),
            should_color,
        }
    }

    pub fn red(mut self) -> Self {
        if self.should_color {
            self.text = self.text.red().to_string();
        }
        self
    }

    pub fn green(mut self) -> Self {
        if self.should_color {
            self.text = self.text.green().to_string();
        }
        self
    }

    pub fn blue(mut self) -> Self {
        if self.should_color {
            self.text = self.text.blue().to_string();
        }
        self
    }

    pub fn yellow(mut self) -> Self {
        if self.should_color {
            self.text = self.text.yellow().to_string();
        }
        self
    }

    pub fn cyan(mut self) -> Self {
        if self.should_color {
            self.text = self.text.cyan().to_string();
        }
        self
    }

    pub fn magenta(mut self) -> Self {
        if self.should_color {
            self.text = self.text.magenta().to_string();
        }
        self
    }

    pub fn bold(mut self) -> Self {
        if self.should_color {
            self.text = self.text.bold().to_string();
        }
        self
    }

    pub fn dimmed(mut self) -> Self {
        if self.should_color {
            self.text = self.text.dimmed().to_string();
        }
        self
    }
}

impl std::fmt::Display for ChainedStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_mode_parsing() {
        assert_eq!("auto".parse::<ColorMode>().unwrap(), ColorMode::Auto);
        assert_eq!("always".parse::<ColorMode>().unwrap(), ColorMode::Always);
        assert_eq!("never".parse::<ColorMode>().unwrap(), ColorMode::Never);
        assert!("invalid".parse::<ColorMode>().is_err());
    }

    #[test]
    fn test_color_helper_never() {
        let helper = ColorHelper::new(ColorMode::Never);
        assert!(!helper.should_color_stdout());
        assert!(!helper.should_color_stderr());
    }

    #[test]
    fn test_color_helper_always() {
        let helper = ColorHelper::new(ColorMode::Always);
        // Should be true unless NO_COLOR is set
        if !helper.no_color {
            assert!(helper.should_color_stdout());
            assert!(helper.should_color_stderr());
        }
    }
}