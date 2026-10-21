//! Reading `dokt.toml`: every setting, its default, and the checks that the
//! TOML schema alone cannot express (color syntax, marker words, etc.).

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

mod defaults;
pub use defaults::example_config;
use defaults::{
    yes,
    default_include,
    default_out_dir,
};

/// The config file's name. It is looked for in the current directory, then in each parent.
pub const CONFIG_NAME: &str = "dokt.toml";

/// How TODO markers are found and collected.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Todos {
    pub enabled: bool,
    /// Whole words (case-sensitive) that mark a TODO.
    pub markers: Vec<String>,
    /// File names (not paths) whose list items all count as TODOs.
    pub files: Vec<String>,
}

impl Default for Todos {
    fn default() -> Self {
        Todos {
            enabled: true,
            markers: vec!["TODO".into()],
            files: vec!["TODO.md".into()],
        }
    }
}

/// Which color palette(s) the site uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Follow the operating system's light/dark setting.
    #[default]
    Auto,
    Light,
    Dark,
}

/// Color overrides for one scheme. Anything left out uses the built-in default.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Palette {
    pub bg: Option<String>,
    pub fg: Option<String>,
    pub muted: Option<String>,
    pub line: Option<String>,
    pub soft: Option<String>,
    pub accent: Option<String>,
    pub broken: Option<String>,
    pub todo: Option<String>,
}

/// The `[theme]` table: light/dark mode, the theme button, and color overrides.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Theme {
    pub mode: Mode,
    /// Show a button to switch between auto, light and dark. Only applies
    /// when `mode` is "auto". Also controls the `t` shortcut
    pub toggle: bool,
    pub light: Palette,
    pub dark: Palette,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            mode: Mode::Auto,
            toggle: true,
            light: Palette::default(),
            dark: Palette::default(),
        }
    }
}

/// Branding elements, like logo, favicon and footer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Brand {
    /// Image shown next to the site title.
    pub logo: Option<String>,
    /// Used instead of `logo` in the dark theme.
    pub logo_dark: Option<String>,
    /// Browser tab icon. Defaults to `logo`.
    pub favicon: Option<String>,
    /// `false` shows only the logo (the title stays for screen readers).
    pub show_title: bool,
    /// Plain text shown at the bottom of every page.
    pub footer: Option<String>,
}

impl Default for Brand {
    fn default() -> Self {
        Brand {
            logo: None,
            logo_dark: None,
            favicon: None,
            show_title: true,
            footer: None,
        }
    }
}

/// Paper size for the PDF export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
#[value(rename_all = "lower")]
pub enum Paper {
    #[default]
    A4,
    Letter,
}

/// Settings for the PDF export
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Export {
    pub paper: Paper,
    /// Path to a Chrome, Chromium or Edge executable. Found automatically if unset.
    pub browser: Option<String>,
}

/// Image types accepted for `brand.logo` and `brand.logo_dark`.
pub const LOGO_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];
/// Image types accepted for `brand.favicon`.
pub const FAVICON_EXTS: &[&str] = &["ico", "png", "svg", "gif", "jpg", "jpeg", "webp", "avif"];

/// Represents a complete `dokt.toml` config file.
///
/// Every field has a default, so an empty file is valid. Unknown keys are
/// rejected, so a typo fails loudly instead of being dropped silently
/// All paths are relative to the config file
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Site title shown in the sidebar and page titles
    #[serde(default)]
    pub title: Option<String>,
    /// Where the generated site is written
    #[serde(default = "default_out_dir")]
    pub out_dir: String,
    /// Glob patterns of files to consider
    #[serde(default = "default_include")]
    pub include: Vec<String>,
    /// gitignore-style patterns of files/directories to skip. Matching
    /// directories are never entered, so excluded trees cost nothing
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Also skip `node_modules/`, `.git/` and `/target/`
    #[serde(default = "yes")]
    pub default_excludes: bool,
    /// Honour `.gitignore` files found while crawling
    #[serde(default = "yes")]
    pub respect_gitignore: bool,
    /// Pass raw HTML in Markdown through to the output
    #[serde(default = "yes")]
    pub allow_html: bool,
    /// Extra stylesheets, linked after the built-in styles, in this order
    #[serde(default)]
    pub css: Vec<String>,
    #[serde(default)]
    pub todos: Todos,
    #[serde(default)]
    pub theme: Theme,
    #[serde(default)]
    pub brand: Brand,
    #[serde(default)]
    pub export: Export,
}

/// A loaded config together with where it was found.
pub struct Loaded {
    pub config: Config,
    pub path: PathBuf,
    /// Directory containing the config file; all paths are relative to it.
    pub root: PathBuf,
}

impl Config {
    /// Checks that cannot be expressed in the TOML schema itself.
    pub fn validate(&self) -> Result<()> {
        crate::theme::validate(&self.theme)?;
        let ext_ok = |path: &str, allowed: &[&str]| {
            std::path::Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .map_or(false, |e| allowed.contains(&e.to_ascii_lowercase().as_str()))
        };
        for (key, value, allowed) in [
            ("logo", &self.brand.logo, LOGO_EXTS),
            ("logo_dark", &self.brand.logo_dark, LOGO_EXTS),
            ("favicon", &self.brand.favicon, FAVICON_EXTS),
        ] {
            if let Some(v) = value {
                if !ext_ok(v, allowed) {
                    bail!("brand.{key} = \"{v}\" must be an image file ({})", allowed.join(", "));
                }
            }
        }
        if self.brand.logo_dark.is_some() && self.brand.logo.is_none() {
            bail!("brand.logo_dark needs brand.logo as well (it is the light-theme and fallback logo)");
        }
        for marker in &self.todos.markers {
            let mut chars = marker.chars();
            let ok = chars.next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false)
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !ok {
                bail!("todos.markers entry \"{marker}\" must be a single word (letters, digits, underscore)");
            }
        }
        for file in &self.todos.files {
            if file.is_empty() || file.contains('/') || file.contains('\\') {
                bail!("todos.files entry \"{file}\" must be a file name such as \"TODO.md\", not a path");
            }
        }
        Ok(())
    }

    /// The title to show: the configured one, else the project folder's name.
    pub fn site_title(&self, root: &Path) -> String {
        self.title.clone().unwrap_or_else(|| {
            root.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Docs".into())
        })
    }
}

/// Load the config at `explicit`, or search upwards from the current directory.
pub fn load(explicit: Option<&Path>) -> Result<Loaded> {
    let path = match explicit {
        Some(p) => p.to_path_buf(),
        None => find_upwards()?,
    };
    let path = path
        .canonicalize()
        .with_context(|| format!("cannot open {}", path.display()))?;
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    let config: Config =
        toml::from_str(&text).with_context(|| format!("invalid config {}", path.display()))?;
    config
        .validate()
        .with_context(|| format!("invalid config {}", path.display()))?;
    let root = path.parent().unwrap().to_path_buf();
    Ok(Loaded { config, path, root })
}

/// Look for the config file in the current directory, then in each parent directory.
fn find_upwards() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join(CONFIG_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
        // Helpful for projects that used the tool's earlier name.
        if dir.join("repodocs.toml").is_file() {
            bail!("found repodocs.toml in {}; rename it to {CONFIG_NAME}", dir.display());
        }
        if !dir.pop() {
            bail!("no {CONFIG_NAME} found here or in any parent directory; run `dokt init` first");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config> {
        let c: Config = toml::from_str(text)?;
        c.validate()?;
        Ok(c)
    }

    #[test]
    fn the_generated_example_parses_and_equals_the_defaults() {
        let c = parse(&example_config()).expect("example config must be valid");
        assert_eq!(c.out_dir, "dokt-out");
        assert!(c.todos.enabled && c.todos.markers == ["TODO"]);
        assert_eq!(c.theme.mode, Mode::Auto);
        assert_eq!(c.theme.light.accent.as_deref(), Some("#0a5adf"));
        assert_eq!(c.theme.dark.bg.as_deref(), Some("#14171a"));
        // Stating the defaults explicitly must not change the generated CSS.
        assert_eq!(crate::theme::css(&c.theme), crate::theme::css(&Theme::default()));
    }

    #[test]
    fn partial_palettes_and_unknown_keys() {
        let c = parse("[theme.light]\naccent = \"#d6336c\"\n").unwrap();
        assert_eq!(c.theme.light.accent.as_deref(), Some("#d6336c"));
        assert!(c.theme.light.bg.is_none());
        assert!(parse("[theme.light]\nacent = \"#fff\"\n").is_err());
        assert!(parse("[theme]\nmode = \"sepia\"\n").is_err());
        assert!(parse("[todos]\nmarker = [\"TODO\"]\n").is_err());
    }

    #[test]
    fn rejects_bad_colors_markers_and_todo_files() {
        let e = parse("[theme.dark]\nbg = \"red; } *{display:none\"\n").unwrap_err().to_string();
        assert!(e.contains("theme.dark.bg"), "{e}");
        assert!(parse("[todos]\nmarkers = [\"TO DO\"]\n").is_err());
        assert!(parse("[todos]\nmarkers = [\"\"]\n").is_err());
        assert!(parse("[todos]\nfiles = [\"docs/TODO.md\"]\n").is_err());
        assert!(parse("[todos]\nmarkers = [\"TODO\", \"FIXME\", \"NOTE_2\"]\n").is_ok());
    }
}
