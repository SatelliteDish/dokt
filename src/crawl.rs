//! Finding the files to publish.
//!
//! A walk over the project that applies the `include` globs, the gitignore-style
//! `exclude` patterns and any `.gitignore` files. Excluded directories are pruned
//! during the walk, so something like `node_modules` is never read at all.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// What a crawled file is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Page,
    Asset,
}

/// One file found by the crawl.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Path relative to the project root, using `/` separators.
    pub rel: String,
    pub abs: PathBuf,
    pub kind: Kind,
    pub mtime: Option<SystemTime>,
    pub len: u64,
}

/// A file named in the config (stylesheet, logo, favicon) that is copied into
/// the site and watched by `serve`.
#[derive(Debug, Clone)]
pub struct ExtraFile {
    pub abs: PathBuf,
    /// File name, used to name the copy in the output.
    pub name: String,
    pub mtime: Option<SystemTime>,
    pub len: u64,
}

/// The logo and favicon files named under `[brand]`.
#[derive(Debug, Clone, Default)]
pub struct BrandFiles {
    pub logo: Option<ExtraFile>,
    pub logo_dark: Option<ExtraFile>,
    pub favicon: Option<ExtraFile>,
}

/// The result of a crawl: the project's files, plus the files the config names
/// (stylesheets, logo, favicon), which `serve` also watches for changes.
pub struct Crawl {
    pub files: Vec<FileEntry>,
    pub styles: Vec<ExtraFile>,
    pub brand: BrandFiles,
}

impl Crawl {
    /// A cheap fingerprint used by `serve` to notice changes.
    pub fn fingerprint(&self) -> Vec<(String, Option<SystemTime>, u64)> {
        let mut fp: Vec<_> = self.files.iter().map(|f| (f.rel.clone(), f.mtime, f.len)).collect();
        let brand = [&self.brand.logo, &self.brand.logo_dark, &self.brand.favicon];
        fp.extend(
            self.styles
                .iter()
                .chain(brand.into_iter().flatten())
                .map(|s| (format!("\u{0}extra:{}", s.abs.display()), s.mtime, s.len)),
        );
        fp
    }
}

/// Check that a file named in the config exists, and record its size and
/// modification time so `serve` can notice when it changes.
fn resolve_file(root: &Path, entry: &str, what: &str) -> Result<ExtraFile> {
    let path = Path::new(entry);
    let abs = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
    let meta = std::fs::metadata(&abs)
        .with_context(|| format!("{what} `{entry}` was not found at {}", abs.display()))?;
    if !meta.is_file() {
        bail!("{what} `{entry}` is not a file");
    }
    Ok(ExtraFile {
        name: abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        abs,
        mtime: meta.modified().ok(),
        len: meta.len(),
    })
}

/// Locate the files named under `[brand]`.
fn resolve_brand(root: &Path, cfg: &Config) -> Result<BrandFiles> {
    let get = |value: &Option<String>, key: &str| -> Result<Option<ExtraFile>> {
        value.as_deref().map(|v| resolve_file(root, v, &format!("brand.{key}"))).transpose()
    };
    Ok(BrandFiles {
        logo: get(&cfg.brand.logo, "logo")?,
        logo_dark: get(&cfg.brand.logo_dark, "logo_dark")?,
        favicon: get(&cfg.brand.favicon, "favicon")?,
    })
}

/// Locate the stylesheets listed in `css`, in order.
fn resolve_styles(root: &Path, cfg: &Config) -> Result<Vec<ExtraFile>> {
    let mut styles = Vec::new();
    for entry in &cfg.css {
        let path = Path::new(entry);
        let abs = if path.is_absolute() { path.to_path_buf() } else { root.join(path) };
        let is_css = abs
            .extension()
            .and_then(|e| e.to_str())
            .map_or(false, |e| e.eq_ignore_ascii_case("css"));
        if !is_css {
            bail!("css entry `{entry}` must be a .css file");
        }
        let meta = std::fs::metadata(&abs)
            .with_context(|| format!("css file `{entry}` (from `css` in the config) was not found at {}", abs.display()))?;
        if !meta.is_file() {
            bail!("css entry `{entry}` is not a file");
        }
        styles.push(ExtraFile {
            name: abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            abs,
            mtime: meta.modified().ok(),
            len: meta.len(),
        });
    }
    Ok(styles)
}

/// File extensions that are Markdown pages.
const PAGE_EXTS: &[&str] = &["md", "markdown"];

/// Classify a path by its extension; `None` for files dokt does not publish.
fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if PAGE_EXTS.contains(&ext.as_str()) {
        Some(Kind::Page)
    } else if crate::markdown::is_asset(&ext) {
        Some(Kind::Asset)
    } else {
        None
    }
}

/// Compile the `include` globs. `*` stays within a folder, `**` crosses folders.
fn build_includes(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_start_matches("./");
        let g = GlobBuilder::new(p)
            .literal_separator(true)
            .build()
            .with_context(|| format!("bad include glob `{p}`"))?;
        b.add(g);
    }
    Ok(b.build()?)
}

/// Compile the `exclude` patterns (plus the default excludes) with gitignore
/// semantics, so `drafts/` and `/vendor/` mean what they mean in a `.gitignore`.
fn build_excludes(root: &Path, cfg: &Config) -> Result<Gitignore> {
    let mut b = GitignoreBuilder::new(root);
    if cfg.default_excludes {
        for line in ["node_modules/", ".git/", "/target/"] {
            b.add_line(None, line)?;
        }
    }
    for line in &cfg.exclude {
        b.add_line(None, line)
            .with_context(|| format!("bad exclude pattern `{line}`"))?;
    }
    Ok(b.build()?)
}

/// Walk the project and return every included page and asset.
///
/// Excluded directories (and anything matched by `.gitignore`) are pruned
/// during the walk rather than filtered afterwards, so `node_modules` is
/// never read.
pub fn crawl(root: &Path, cfg: &Config, out_dir: &Path) -> Result<Crawl> {
    let include = build_includes(&cfg.include)?;
    let exclude = build_excludes(root, cfg)?;
    let out_dir = out_dir.to_path_buf();

    let mut walker = WalkBuilder::new(root);
    walker
        .hidden(true) // skip dotfiles and dot-directories
        .follow_links(false)
        .parents(false)
        .require_git(false)
        .ignore(false)
        .git_global(false)
        .git_ignore(cfg.respect_gitignore)
        .git_exclude(cfg.respect_gitignore);

    let exclude_for_filter = exclude.clone();
    walker.filter_entry(move |entry| {
        let path = entry.path();
        if path.starts_with(&out_dir) {
            return false;
        }
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        !exclude_for_filter.matched(path, is_dir).is_ignore()
    });

    let mut files = Vec::new();
    for result in walker.build() {
        let entry = match result {
            Ok(e) => e,
            Err(err) => {
                eprintln!("warning: {err}");
                continue;
            }
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let Some(kind) = kind_of(entry.path()) else { continue };
        let Ok(rel_path) = entry.path().strip_prefix(root) else { continue };
        let rel = rel_path
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if !include.is_match(&rel) {
            continue;
        }
        let meta = entry.metadata().ok();
        files.push(FileEntry {
            rel,
            abs: entry.path().to_path_buf(),
            kind,
            mtime: meta.as_ref().and_then(|m| m.modified().ok()),
            len: meta.map(|m| m.len()).unwrap_or(0),
        });
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));
    let styles = resolve_styles(root, cfg)?;
    let brand = resolve_brand(root, cfg)?;
    Ok(Crawl { files, styles, brand })
}
