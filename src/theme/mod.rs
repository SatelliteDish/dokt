//! The color palette: defaults, validation of user overrides, and the
//! generated `assets/theme.css`.
//!
//! The defaults live here and nowhere else. `dokt init` writes them into
//! the example config, and the CSS is generated from them, so they cannot drift.

use crate::config::{Mode, Palette, Theme};
use anyhow::{bail, Result};

/// Palette keys, in the order shown in generated files.
pub const KEYS: [&str; 8] = [
    "bg",      // page background
    "fg",      // body text
    "muted",   // secondary text
    "line",    // borders and dividers
    "soft",    // sidebar, code blocks, table headers
    "accent",  // links and the current page
    "broken",  // links to pages that do not exist
    "todo"     // background of highlighted TODO markers
];
/// Default light palette, in the order of [`KEYS`].
pub const LIGHT: [&str; 8] = [
    "#ffffff", // bg
    "#1f2328", // fg
    "#59636e", // muted
    "#d8dee4", // line
    "#f4f6f8", // soft
    "#0a5adf", // accent
    "#b42318", // broken
    "#fff1b3", // todo
];
/// Default dark palette, in the order of [`KEYS`].
pub const DARK: [&str; 8] = [
    "#14171a", // bg
    "#dde2e7", // fg
    "#96a1ad", // muted
    "#2c333a", // line
    "#1c2126", // soft
    "#79aaff", // accent
    "#ff8a80", // broken
    "#5a4a00", // todo
];

impl Palette {
    /// The override for a palette key, if one was set.
    pub fn get(&self, key: &str) -> Option<&str> {
        match key {
            "bg" => self.bg.as_deref(),
            "fg" => self.fg.as_deref(),
            "muted" => self.muted.as_deref(),
            "line" => self.line.as_deref(),
            "soft" => self.soft.as_deref(),
            "accent" => self.accent.as_deref(),
            "broken" => self.broken.as_deref(),
            "todo" => self.todo.as_deref(),
            _ => None,
        }
    }
}

/// Accepts `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `transparent`, and
/// `rgb() / rgba() / hsl() / hsla() / hwb() / lab() / lch() / oklab() / oklch() / color()`.
pub fn is_valid_color(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || v.len() > 100 {
        return false;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit());
    }
    let lower = v.to_ascii_lowercase();
    if lower == "transparent" {
        return true;
    }
    const FUNCTIONS: [&str; 10] = [
        "rgb(", "rgba(", "hsl(", "hsla(", "hwb(", "lab(", "lch(", "oklab(", "oklch(", "color(",
    ];
    if !FUNCTIONS.iter().any(|f| lower.starts_with(f)) || !lower.ends_with(')') {
        return false;
    }
    let open = lower.find('(').unwrap();
    let inner = &lower[open + 1..lower.len() - 1];
    !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || " .,/%-+".contains(c))
}

/// Reject any color override that is not a safe, recognisable color.
pub fn validate(theme: &Theme) -> Result<()> {
    for (name, palette) in [("light", &theme.light), ("dark", &theme.dark)] {
        for key in KEYS {
            if let Some(v) = palette.get(key) {
                if !is_valid_color(v) {
                    bail!(
                        "theme.{name}.{key} = \"{v}\" is not a usable color; use a hex value like #0a5adf, \
                         or rgb() / hsl() / oklch()"
                    );
                }
            }
        }
    }
    Ok(())
}

/// CSS custom property declarations for one palette: overrides where set, else the defaults.
fn declarations(defaults: &[&str; 8], palette: &Palette, indent: &str) -> String {
    KEYS.iter()
        .zip(defaults.iter())
        .map(|(key, default)| {
            let value = palette.get(key).map(str::trim).unwrap_or(default);
            format!("{indent}--{key}: {value};\n")
        })
        .collect()
}

/// The contents of `assets/theme.css`.
pub fn css(theme: &Theme) -> String {
    let mut out = String::from("/* Generated from [theme] in dokt.toml. Do not edit. */\n");
    match theme.mode {
        Mode::Auto => {
            out.push_str(":root {\n  color-scheme: light dark;\n");
            out.push_str(&declarations(&LIGHT, &theme.light, "  "));
            // Without a manual choice (no data-theme) the OS decides...
            out.push_str("}\n@media (prefers-color-scheme: dark) {\n  :root:not([data-theme=\"light\"]) {\n");
            out.push_str(&declarations(&DARK, &theme.dark, "    "));
            out.push_str("  }\n}\n");
            // ...and the theme button's choice (data-theme on <html>) overrides it.
            out.push_str(":root[data-theme=\"light\"] {\n  color-scheme: light;\n}\n");
            out.push_str(":root[data-theme=\"dark\"] {\n  color-scheme: dark;\n");
            out.push_str(&declarations(&DARK, &theme.dark, "  "));
            out.push_str("}\n");
        }
        Mode::Light => {
            out.push_str(":root {\n  color-scheme: light;\n");
            out.push_str(&declarations(&LIGHT, &theme.light, "  "));
            out.push_str("}\n");
        }
        Mode::Dark => {
            out.push_str(":root {\n  color-scheme: dark;\n");
            out.push_str(&declarations(&DARK, &theme.dark, "  "));
            out.push_str("}\n");
        }
    }
    out
}

/// `key = "value"` lines for the example config.
pub fn example_lines(defaults: &[&str; 8]) -> String {
    KEYS.iter()
        .zip(defaults.iter())
        .map(|(key, value)| format!("{key:<7} = \"{value}\"\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_validation() {
        for ok in ["#fff", "#FFFF", "#0a5adf", "#0a5adf80", "rgb(10 20 30 / 50%)", "rgba(1,2,3,.5)",
                   "hsl(210, 80%, 45%)", "oklch(60% 0.2 250)", "transparent", " #abc "] {
            assert!(is_valid_color(ok), "{ok} should be valid");
        }
        for bad in ["", "red", "#ff", "#ggg", "#12345", "red; } body { display: none", "rgb(1,2,3); x",
                    "url(http://x)", "rgb(var(--x))", "rgb(", "foo(1,2,3)", "expression(1)", "\"#fff\""] {
            assert!(!is_valid_color(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn defaults_are_all_valid() {
        for v in LIGHT.iter().chain(DARK.iter()) {
            assert!(is_valid_color(v), "{v}");
        }
    }

    #[test]
    fn auto_mode_has_light_root_and_dark_media_query() {
        let mut t = Theme::default();
        t.light.accent = Some("#d6336c".into());
        let css = css(&t);
        assert!(css.contains("--accent: #d6336c;"));
        assert!(css.contains("--bg: #ffffff;"));
        assert!(css.contains("@media (prefers-color-scheme: dark)"));
        assert!(css.contains("--bg: #14171a;")); // dark default untouched
    }

    #[test]
    fn auto_mode_lets_a_manual_choice_override_the_os() {
        let css = css(&Theme::default());
        // dark via the OS, unless the user picked light
        assert!(css.contains(r#"@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {"#), "{css}");
        // explicit dark must work even when the OS is light
        let dark_block = css.split(r#":root[data-theme="dark"] {"#).nth(1).expect("explicit dark rule");
        assert!(dark_block.contains("color-scheme: dark;") && dark_block.contains("--bg: #14171a;"));
        assert!(css.contains(r#":root[data-theme="light"] {
  color-scheme: light;"#));
    }

    #[test]
    fn forced_modes_emit_a_single_palette() {
        let mut t = Theme::default();
        t.mode = Mode::Dark;
        let css = css(&t);
        assert!(!css.contains("@media"));
        assert!(css.contains("color-scheme: dark;") && css.contains("--bg: #14171a;"));
        assert!(!css.contains("#ffffff"));
        t.mode = Mode::Light;
        let css = super::css(&t);
        assert!(!css.contains("@media") && css.contains("--bg: #ffffff;") && !css.contains("#14171a"));
    }

    #[test]
    fn bad_override_is_reported_with_its_path() {
        let mut t = Theme::default();
        t.dark.fg = Some("nope".into());
        let err = validate(&t).unwrap_err().to_string();
        assert!(err.contains("theme.dark.fg"), "{err}");
    }
}
