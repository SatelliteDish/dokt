/// Default value for `out_dir`.
pub fn default_out_dir() -> String {
    "dokt-out".into()
}
/// Default for `include`
pub fn default_include() -> Vec<String> {
    vec!["**".into()] // Include everything
}
/// Serde default for settings that are on unless turned off.
pub fn yes() -> bool {
    true
}

/// The text written by the `init` command. The palette lines come from [`crate::theme`], so the
/// example file always shows the real defaults.
pub fn example_config() -> String {
    format!(
        r##"# dokt configuration. All paths are relative to this file.

# Shown in the sidebar and in page titles. Defaults to the folder name.
# title = "My Project"

# Where the generated site goes. dokt refuses to write into a
# non-empty directory it did not create itself.
out_dir = "dokt-out"

# Globs of files to consider. Markdown files become pages; images and PDFs
# that match are copied so relative references keep working.
include = ["**"]
# include = ["README.md", "docs/**", "packages/*/README.md"]

# gitignore-style patterns to skip. Matching directories are never entered.
exclude = [
  # "CHANGELOG.md",
  # "docs/drafts/",
]

# Also skip node_modules/, .git/ and /target/ (recommended).
default_excludes = true

# Honour .gitignore files found while crawling.
respect_gitignore = true

# Let raw HTML inside Markdown through to the output.
allow_html = true

# Your own stylesheets. They are copied into the site and linked after the
# built-in styles, in this order, so anything you write here wins.
# Files they reference (fonts, images) are not copied; use absolute URLs.
# Handy variables: --sidebar (width), --sans, --mono, plus the colors below.
css = [
  # "branding/docs.css",
]

# Highlight TODO markers in pages and collect them on a TODOs page.
[todos]
enabled = true
# Whole words, case-sensitive. "TODOs" and "autodoc" do not match. Add "FIXME" if you like.
markers = ["TODO"]
# File names whose list items ALL count as TODOs (checked items count as done).
files = ["TODO.md"]

# Branding. Paths are relative to this file; the files are copied into the site.
[brand]
# logo      = "branding/logo.svg"       # shown next to the site title
# logo_dark = "branding/logo-dark.svg"  # optional: used instead in the dark theme
# favicon   = "branding/favicon.png"    # optional: defaults to the logo
show_title = true                       # false shows only the logo
# footer    = "(c) 2026 Your Name"      # plain text at the bottom of every page

# PDF export (`dokt export --pdf`). The PDF needs Chrome, Chromium or Edge installed.
[export]
paper = "a4"          # "a4" or "letter"
# browser = "/usr/bin/chromium"   # only if it is not found automatically

# Colors. Delete any line to fall back to the same default shown here.
# mode:   "auto" follows the OS light/dark setting; "light" or "dark" forces one.
# toggle: show a button (and the `t` key) to switch between auto, light and dark.
#         Only applies when mode is "auto".
# Values must be hex (#0a5adf) or rgb()/hsl()/oklch() style.
[theme]
mode = "auto"
toggle = true

[theme.light]
{light}
[theme.dark]
{dark}"##,
        light = crate::theme::example_lines(&crate::theme::LIGHT),
        dark = crate::theme::example_lines(&crate::theme::DARK),
    )
}
