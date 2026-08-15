use colored::{ColoredString, Colorize};
use is_terminal::IsTerminal;
use std::env;
use std::io::{stderr, stdout};

/// Serializes every test in this crate that manipulates
/// `colored::control::SHOULD_COLORIZE` — a process-global flag, so any two
/// tests that touch it concurrently race each other's override regardless
/// of which module each test lives in.
///
/// This crate used to have two of these locks, one private to this module
/// and a second, unrelated `Mutex` in `display.rs`'s own test module. Two
/// separate locks guard nothing against each other: `display.rs`'s
/// `pad_then_colorize` test could set the global override at the same
/// moment a test here read or reset it, each holding a *different* lock
/// and believing itself alone. Filtering to the four tests that touch the
/// global and running them at `--test-threads=8` reproduced the resulting
/// "colorized string carries no escape bytes" failure in 8 of 40 runs.
/// Every module that touches the global now imports this one lock instead.
#[cfg(test)]
pub(crate) static COLORIZE_OVERRIDE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Color mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    #[default]
    Auto, // Use TTY detection
    Always, // Always use colors
    Never,  // Never use colors
}

impl ColorMode {
    /// Force `colored`'s own global colorize gate to agree with this mode.
    ///
    /// `colored`'s `Colorize` methods (`.red()`, `.bold()`, ...) decide
    /// whether to actually emit ANSI escapes from a process-global flag,
    /// checked at display time — independent of whatever a [`ColorHelper`]
    /// decided. Without this, `--color always` piped to a file was a
    /// no-op: `ColorHelper::should_color_stdout` returned `true`, the code
    /// called `.red()`, and `colored` stripped the escapes right back out
    /// because *its own* TTY check said stdout wasn't a terminal.
    pub fn apply_as_global_override(self) {
        match self {
            ColorMode::Always => colored::control::set_override(true),
            ColorMode::Never => colored::control::set_override(false),
            ColorMode::Auto => colored::control::unset_override(),
        }
    }
}

impl std::str::FromStr for ColorMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(ColorMode::Auto),
            "always" => Ok(ColorMode::Always),
            "never" => Ok(ColorMode::Never),
            _ => Err(format!(
                "Invalid color mode: '{}'. Valid options: auto, always, never",
                s
            )),
        }
    }
}

/// TTY-aware color helper that respects NO_COLOR and terminal detection.
///
/// Tracks stdout and stderr terminal-ness *independently*, because a
/// caller can redirect either stream on its own (`prog 2>err.log`,
/// `prog >out.log`) and the two must be judged separately — gating both on
/// stdout's TTY state left colored escapes in a redirected stderr file, or
/// stripped them from a stderr that was still an interactive terminal.
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
        Self::with_terminal_state(mode, stdout().is_terminal(), stderr().is_terminal())
    }

    /// Create a color helper with explicit terminal state, bypassing the
    /// real `is_terminal()` checks. Used by tests, which otherwise have no
    /// way to observe stdout- and stderr-gating as independent — both
    /// streams are captured (non-terminal) under the test harness.
    fn with_terminal_state(
        mode: ColorMode,
        stdout_is_terminal: bool,
        stderr_is_terminal: bool,
    ) -> Self {
        Self {
            mode,
            stdout_is_terminal,
            stderr_is_terminal,
            no_color: env::var("NO_COLOR").is_ok()
                && !env::var("NO_COLOR").unwrap_or_default().is_empty(),
        }
    }

    /// Check if colors should be used for stdout
    pub fn should_color_stdout(&self) -> bool {
        self.should_use_colors(self.stdout_is_terminal)
    }

    /// Check if colors should be used for stderr. Gated on stderr's own
    /// terminal state, never stdout's — see the struct-level note.
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

    /// Apply red color, gated on stderr's terminal state, and return the
    /// finished (possibly ANSI-escaped) string. For text that is about to
    /// be written with `errln!` — user-facing error output must never
    /// be gated on stdout's TTY state (see the struct-level note), or
    /// redirecting only stderr leaves raw escape sequences in the file.
    ///
    /// Returns an owned `String`, not a `ColoredString`, because the
    /// escape decision has to be finalized — via
    /// [`force_override_for_stderr`](Self::force_override_for_stderr) —
    /// before this function returns. `colored` only consults its global
    /// flag when a `ColoredString` is displayed, so handing one back to
    /// the caller would let the override guard drop, and the flag revert,
    /// before `.to_string()` ever ran.
    pub fn red_err(&self, text: &str) -> String {
        let _guard = self.force_override_for_stderr();
        if self.should_color_stderr() {
            text.red().to_string()
        } else {
            text.normal().to_string()
        }
    }

    /// Chainable color and formatting methods, gated on stderr.
    pub fn style_err(&self) -> StyleBuilder {
        StyleBuilder::new_for_stderr(self.should_color_stderr(), self.force_override_for_stderr())
    }

    /// Force `colored`'s process-global colorize flag to match this
    /// helper's own (stream-aware) stderr decision, returning a guard that
    /// restores `self.mode`'s own global setting when dropped.
    ///
    /// `colored` 3.1.1 computes its Auto-mode flag once, from **stdout's**
    /// terminal state alone (`ShouldColorize::from_env`, `colored`'s
    /// `control.rs`), and consults only that global — never this struct's
    /// own `should_color_stderr` — when a `ColoredString` is finally
    /// displayed. Left alone, `--color auto` with a piped stdout and a
    /// real terminal on stderr silently strips stderr's escape codes,
    /// because the global flag never looked at stderr at all (#58.5 fixed
    /// the *decision*; this fixes the bytes actually reaching the global
    /// gate `colored` checks at Display time).
    ///
    /// `Always`/`Never` already leave a persistent, stream-agnostic
    /// override in place from [`ColorMode::apply_as_global_override`], so
    /// forcing and restoring it here is a harmless no-op for those modes —
    /// only `Auto` ever observes a different value while the guard is
    /// alive.
    fn force_override_for_stderr(&self) -> RestoreOverrideGuard {
        colored::control::set_override(self.should_color_stderr());
        RestoreOverrideGuard(self.mode)
    }
}

/// Restores `colored`'s process-global override to whatever `ColorMode`
/// establishes, when dropped. See
/// [`ColorHelper::force_override_for_stderr`].
struct RestoreOverrideGuard(ColorMode);

impl Drop for RestoreOverrideGuard {
    fn drop(&mut self) {
        self.0.apply_as_global_override();
    }
}

/// Builder for chaining color and formatting operations
pub struct StyleBuilder {
    should_color: bool,
    /// Set only by [`ColorHelper::style_err`] — carried through into
    /// [`ChainedStyle`] so the override it forces stays in effect for the
    /// whole `.red(text).bold()...` chain, and is only released once the
    /// final `ChainedStyle` is dropped (after `.to_string()`/`Display` has
    /// already run). `style()` (the stdout variant) never sets this:
    /// `colored`'s own Auto-mode detection is already stdout-based, so
    /// stdout needs no forcing — see `force_override_for_stderr`.
    _guard: Option<RestoreOverrideGuard>,
}

impl StyleBuilder {
    pub fn new(should_color: bool) -> Self {
        Self {
            should_color,
            _guard: None,
        }
    }

    fn new_for_stderr(should_color: bool, guard: RestoreOverrideGuard) -> Self {
        Self {
            should_color,
            _guard: Some(guard),
        }
    }

    pub fn red(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color, self._guard).red()
    }

    pub fn green(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color, self._guard).green()
    }

    pub fn blue(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color, self._guard).blue()
    }

    pub fn yellow(self, text: &str) -> ChainedStyle {
        ChainedStyle::new(text, self.should_color, self._guard).yellow()
    }
}

/// Chainable style operations
pub struct ChainedStyle {
    text: String,
    should_color: bool,
    /// See [`StyleBuilder::_guard`]. Carried along by every builder method
    /// below (each takes and returns `Self`), so it lives exactly as long
    /// as the chain — and is dropped only once the caller's final
    /// `.to_string()` has finished reading `colored`'s global flag.
    _guard: Option<RestoreOverrideGuard>,
}

impl ChainedStyle {
    fn new(text: &str, should_color: bool, guard: Option<RestoreOverrideGuard>) -> Self {
        Self {
            text: text.to_string(),
            should_color,
            _guard: guard,
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

    pub fn bold(mut self) -> Self {
        if self.should_color {
            self.text = self.text.bold().to_string();
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
    }

    #[test]
    fn test_color_helper_always() {
        let helper = ColorHelper::new(ColorMode::Always);
        // Should be true unless NO_COLOR is set
        if !helper.no_color {
            assert!(helper.should_color_stdout());
        }
    }

    // --- stdout/stderr gating are independent (#58.5) ---

    #[test]
    fn stdout_piped_stderr_terminal_colors_stderr_only() {
        let helper = ColorHelper::with_terminal_state(ColorMode::Auto, false, true);
        if !helper.no_color {
            assert!(!helper.should_color_stdout(), "stdout is piped, not a tty");
            assert!(helper.should_color_stderr(), "stderr is a tty");
        }
    }

    #[test]
    fn stderr_piped_stdout_terminal_colors_stdout_only() {
        let helper = ColorHelper::with_terminal_state(ColorMode::Auto, true, false);
        if !helper.no_color {
            assert!(helper.should_color_stdout(), "stdout is a tty");
            assert!(!helper.should_color_stderr(), "stderr is piped, not a tty");
        }
    }

    #[test]
    fn never_mode_disables_both_streams_regardless_of_terminal_state() {
        let helper = ColorHelper::with_terminal_state(ColorMode::Never, true, true);
        assert!(!helper.should_color_stdout());
        assert!(!helper.should_color_stderr());
    }

    #[test]
    fn always_mode_enables_both_streams_regardless_of_terminal_state() {
        let helper = ColorHelper::with_terminal_state(ColorMode::Always, false, false);
        if !helper.no_color {
            assert!(helper.should_color_stdout());
            assert!(helper.should_color_stderr());
        }
    }

    // --- ColorMode forces colored's own global gate (#58.1) ---
    //
    // `colored::control::SHOULD_COLORIZE` is process-global state, so these
    // tests serialize on `COLORIZE_OVERRIDE_TEST_LOCK` (shared with
    // `display.rs`, see its doc comment) to avoid racing each other, and
    // clean up afterward so they don't leak state into unrelated tests.

    #[test]
    fn always_forces_the_global_colorize_override_on() {
        let _guard = COLORIZE_OVERRIDE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ColorMode::Always.apply_as_global_override();
        assert!(colored::control::SHOULD_COLORIZE.should_colorize());
        colored::control::unset_override();
    }

    #[test]
    fn never_forces_the_global_colorize_override_off() {
        let _guard = COLORIZE_OVERRIDE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Start from a state where the ambient default would say "yes,
        // colorize" (`cargo test`'s own piped stdout otherwise makes
        // `colored` default to no-colour regardless of what this test
        // does, which is exactly what made this assertion pass even with
        // the function body deleted). Forcing `true` first means `Never`
        // is the only thing that can make the assertion below succeed.
        colored::control::set_override(true);
        ColorMode::Never.apply_as_global_override();
        assert!(!colored::control::SHOULD_COLORIZE.should_colorize());
        colored::control::unset_override();
    }

    // --- `colored`'s global gate only ever reflects stdout, not stderr
    // (see `ColorHelper::force_override_for_stderr`) ---

    #[test]
    fn auto_mode_colorizes_stderr_even_when_process_stdout_is_not_a_terminal() {
        let _guard = COLORIZE_OVERRIDE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Clean slate: no manual override, so `colored`'s own Auto
        // detection (frozen from this test binary's real stdout — piped
        // under `cargo test`, so "not a terminal") governs unless
        // `red_err` forces it correctly for the duration of the call.
        colored::control::unset_override();

        // stdout piped (false), stderr a real terminal (true) — the exact
        // shape of `data-gov 2>&1 | less`, or any pipeline that only
        // redirects stdout.
        let helper = ColorHelper::with_terminal_state(ColorMode::Auto, false, true);
        let result = helper.red_err("boom");

        colored::control::unset_override();

        if !helper.no_color {
            assert!(
                result.contains('\u{1b}'),
                "stderr is a real terminal under Auto, so red_err must carry \
                 real ANSI escapes even though the process's stdout is piped; \
                 got {result:?}"
            );
        }
    }
}
