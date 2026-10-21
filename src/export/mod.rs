//! `dokt export`: write the docs as a static site (`--web`) and/or one PDF (`--pdf`).
//!
//! The PDF is built from the same Markdown as the site, in a single document:
//!
//! 1. a cover page (title, logo, page count, date),
//! 2. a table of contents laid out like the sidebar, with clickable entries,
//! 3. every page, in sidebar order, each in an `<article>` starting on a new sheet
//!    (the site's typography rules are scoped to `article`, so this keeps the look identical),
//! 4. an appendix collecting all TODOs.
//!
//! Links between pages become in-document links. The document is written into
//! a temporary copy of the site (so it can use the same images, stylesheets and
//! Mermaid bundle) and printed by a headless browser; see [`crate::chrome`].

use crate::chrome::{self, PdfJob};
use crate::config::{Config, Loaded, Palette, Paper, Theme, Mode};
use crate::crawl::{self, Crawl};
use crate::markdown::{self, esc, Ctx, Target};
use crate::site::{self, PageTodos, TodoLinks};
use crate::theme;
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The print stylesheet and script, written next to the document when exporting.
const PRINT_CSS: &str = include_str!("../../assets/print.css");
const PRINT_JS: &str = include_str!("../../assets/print.js");

/// Name of the PDF written next to the site by default: `<site-title-slug>.pdf`.
pub fn default_pdf_name(site_title: &str) -> String {
    let slug = site::slug(site_title);
    format!("{}.pdf", if slug.is_empty() { "docs" } else { &slug })
}

/// What the `export` command was asked to do.
pub struct ExportArgs {
    /// `Some(None)` is `--web` with no directory (use `out_dir`); `Some(Some(d))` is `--web d`.
    pub web: Option<Option<PathBuf>>,
    /// Likewise for `--pdf [FILE]`.
    pub pdf: Option<Option<PathBuf>>,
    pub paper: Option<Paper>,
    pub browser: Option<PathBuf>,
    pub no_sandbox: bool,
    pub strict: bool,
}

/// Run the export command.
pub fn run(loaded: Loaded, args: ExportArgs) -> Result<()> {
    if args.web.is_none() && args.pdf.is_none() {
        bail!("nothing to export: pass --web (static site), --pdf, or both");
    }
    let Loaded { config: cfg, root, .. } = loaded;
    let default_out = site::out_dir_for(&root, &cfg, None);
    let mut warnings: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut note = |w: &[String], warnings: &mut Vec<String>| {
        for m in w {
            if seen.insert(m.clone()) {
                warnings.push(m.clone());
            }
        }
    };

    // ---- static site ----
    if let Some(dir) = &args.web {
        let out_dir = site::out_dir_for(&root, &cfg, dir.as_deref());
        let crawled = crawl::crawl(&root, &cfg, &out_dir)?;
        let report = site::build(&root, &cfg, &out_dir, &crawled)?;
        println!(
            "Exported the site: {} pages, {} assets -> {}",
            report.pages,
            report.assets,
            out_dir.display()
        );
        note(&report.warnings, &mut warnings);
    }

    // ---- PDF ----
    if let Some(file) = &args.pdf {
        let name = default_pdf_name(&cfg.site_title(&root));
        let dest = match file {
            Some(p) => {
                let abs = if p.is_absolute() { p.clone() } else { std::env::current_dir()?.join(p) };
                if abs.is_dir() { abs.join(&name) } else { abs }
            }
            None => default_out.join(&name),
        };
        // The default location is inside the output directory. Create it the same
        // way `build` does, so a later `--web` export accepts it.
        if dest.parent() == Some(default_out.as_path()) {
            site::prepare_out_dir(&root, &default_out)?;
        } else if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        let crawled = crawl::crawl(&root, &cfg, &default_out)?;
        let paper = args.paper.unwrap_or(cfg.export.paper);
        let browser_hint = args.browser.as_ref().map(|p| p.to_string_lossy().into_owned());
        let browser = chrome::find_browser(browser_hint.as_deref().or(cfg.export.browser.as_deref()))?;

        let stage = TempDir::new("dokt-export")?;
        site::build(&root, &cfg, stage.path(), &crawled).context("building the export staging copy")?;
        let doc = print_document(&root, &cfg, &crawled, paper)?;
        let theme_css = theme::css(&Theme { mode: Mode::Light, toggle: false, light: cfg.theme.light.clone(), dark: Palette::default() });
        fs::write(stage.path().join("assets/print-theme.css"), theme_css)?;
        fs::write(stage.path().join("assets/print.css"), PRINT_CSS)?;
        fs::write(stage.path().join("assets/print.js"), PRINT_JS)?;
        fs::write(stage.path().join("print.html"), &doc.html)?;

        let url = file_url(&stage.path().join("print.html"));
        let result = chrome::print_to_pdf(
            &browser,
            args.no_sandbox,
            &PdfJob { url: &url, header_html: &header_html(&cfg.site_title(&root)), footer_html: &footer_html() },
        )
        .with_context(|| format!("printing with {}", browser.display()))?;
        fs::write(&dest, &result.bytes).with_context(|| format!("writing {}", dest.display()))?;

        let mut pdf_warnings = doc.warnings;
        if result.diagram_errors > 0 {
            pdf_warnings.push(format!("{} diagram(s) could not be drawn; their source is printed instead", result.diagram_errors));
        }
        note(&pdf_warnings, &mut warnings);
        println!(
            "Exported the PDF: {} pages of content, {} KB -> {}",
            doc.pages,
            result.bytes.len().div_ceil(1024),
            dest.display()
        );
    }

    for w in &warnings {
        eprintln!("warning: {w}");
    }
    if args.strict && !warnings.is_empty() {
        bail!("{} warning(s) in strict mode", warnings.len());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Building the print document
// ---------------------------------------------------------------------------

/// The assembled print document.
pub struct PrintDoc {
    /// The complete HTML document, to be saved as `print.html` at the site root.
    pub html: String,
    pub warnings: Vec<String>,
    /// Number of Markdown pages included.
    pub pages: usize,
}

/// A rendered page, ready to be placed in the document.
struct Section {
    id: String,
    title: String,
    html: String,
}

/// Render every page for print and assemble the single document.
///
/// Pages are rendered with [`Target::Print`], which turns links between pages
/// into in-document anchors and namespaces heading ids per page. They are then
/// placed in sidebar order between a cover and a contents list, followed by the
/// TODO appendix when there is anything to list.
pub fn print_document(root: &Path, cfg: &Config, crawl: &Crawl, paper: Paper) -> Result<PrintDoc> {
    let prep = site::prepare(crawl);
    let mut warnings = prep.warnings.clone();
    let ctx = Ctx { index: &prep.index, root, allow_html: cfg.allow_html, todos: &cfg.todos, target: Target::Print };

    // Sidebar order: in each folder the files, then the sub-folders.
    let rels: Vec<&str> = prep.index.pages.iter().map(|p| p.rel.as_str()).collect();
    let tree = build_tree(&rels);
    let mut order = Vec::new();
    flatten(&tree, &mut order);

    let mut sections: Vec<Option<Section>> = (0..rels.len()).map(|_| None).collect();
    let mut todo_pages: Vec<PageTodos> = Vec::new();
    for &i in &order {
        let info = &prep.index.pages[i];
        let source = match fs::read_to_string(&prep.files[i].abs) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!("{}: could not read ({e})", info.rel));
                continue;
            }
        };
        // Rendered as if the page lived at the document root, so images and
        // other files resolve from there.
        let r = markdown::render(&source, &info.rel, "print.html", &ctx);
        warnings.extend(r.warnings.iter().cloned());
        let title = r.title.clone().unwrap_or_else(|| info.stem.clone());
        // A page without its own heading still needs one to be findable in the contents.
        let heading = if r.has_h1 { String::new() } else { format!("<h1 class=\"doc-title\">{}</h1>\n", esc(&title)) };
        let html = format!(
            "<article class=\"doc-page\" id=\"{id}\">\n<p class=\"doc-path\">{rel}</p>\n{heading}{body}</article>\n",
            id = esc(&info.id),
            rel = esc(&info.rel),
            body = r.html,
        );
        if cfg.todos.enabled {
            let is_file = crate::todo::is_todo_file(&info.rel, &cfg.todos);
            if is_file || !r.todos.is_empty() {
                todo_pages.push(PageTodos {
                    html: info.html.clone(),
                    id: info.id.clone(),
                    rel: info.rel.clone(),
                    title: title.clone(),
                    is_file,
                    entries: r.todos,
                });
            }
        }
        sections[i] = Some(Section { id: info.id.clone(), title, html });
    }

    // ---- TODO appendix ----
    let (mut open, mut done) = (0usize, 0usize);
    for p in &todo_pages {
        for e in &p.entries {
            if e.done { done += 1 } else { open += 1 }
        }
    }
    let appendix = if cfg.todos.enabled && !todo_pages.is_empty() {
        let links = TodoLinks {
            page: &|p| format!("#{}", p.id),
            entry: &|_, e| format!("#{}", e.anchor),
        };
        let body = site::todo_page_body(&todo_pages, "Appendix: TODOs", &links, open, done, &cfg.todos.markers, &cfg.todos.files);
        Some(format!("<article class=\"doc-page\" id=\"appendix-todos\">\n{body}</article>\n"))
    } else {
        None
    };

    // ---- assemble ----
    let site_title = cfg.site_title(root);
    let included = order.iter().filter(|&&i| sections[i].is_some()).count();
    let toc = toc_html(&tree, &sections, appendix.is_some());
    let logo = crawl.brand.logo.as_ref().map(|f| {
        let ext = f.abs.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        format!("assets/brand/logo.{ext}")
    });
    let cover = cover_html(&site_title, logo.as_deref(), included, &today(), cfg.brand.footer.as_deref());

    let mut custom = String::new();
    for (i, s) in crawl.styles.iter().enumerate() {
        custom.push_str(&format!("<link rel=\"stylesheet\" href=\"{}\">\n", site::custom_style_rel(i, &s.name)));
    }
    let body: String = order.iter().filter_map(|&i| sections[i].as_ref()).map(|s| s.html.as_str()).collect();
    let html = format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<link rel="stylesheet" href="assets/style.css">
<link rel="stylesheet" href="assets/print-theme.css">
<link rel="stylesheet" href="assets/print.css">
{custom}<style>@page {{ size: {paper}; margin: 20mm 16mm 22mm; }}</style>
</head>
<body class="print">
{cover}<nav class="toc" aria-label="Contents">
<h1>Contents</h1>
{toc}</nav>
<main>
{body}{appendix}</main>
<script src="assets/print.js"></script>
</body>
</html>
"##,
        title = esc(&site_title),
        paper = match paper { Paper::A4 => "A4", Paper::Letter => "Letter" },
        appendix = appendix.unwrap_or_default(),
    );
    Ok(PrintDoc { html, warnings, pages: included })
}

/// The cover page: logo, title, page count and date, and the brand footer.
fn cover_html(title: &str, logo: Option<&str>, pages: usize, date: &str, footer: Option<&str>) -> String {
    let logo = logo.map(|l| format!("<img class=\"cover-logo\" src=\"{}\" alt=\"\">\n", esc(l))).unwrap_or_default();
    let footer = footer
        .filter(|f| !f.trim().is_empty())
        .map(|f| format!("<p class=\"cover-footer\">{}</p>\n", esc(f)))
        .unwrap_or_default();
    format!(
        "<header class=\"cover\">\n{logo}<h1 class=\"cover-title\">{}</h1>\n<p class=\"cover-meta\">{pages} page{} &middot; {}</p>\n{footer}</header>\n",
        esc(title),
        if pages == 1 { "" } else { "s" },
        esc(date)
    )
}

/// Running header (site title) and footer (page X / Y) for every sheet.
/// The browser renders these in a tiny context of their own, so styles are inline.
fn header_html(title: &str) -> String {
    format!(
        "<div style=\"width:100%;padding:0 16mm;font-size:8px;color:#6b7280;\"><span>{}</span></div>",
        esc(title)
    )
}

/// The running footer: `page / total`, filled in by the browser.
fn footer_html() -> String {
    "<div style=\"width:100%;padding:0 16mm;font-size:8px;color:#6b7280;text-align:right;\">\
     <span class=\"pageNumber\"></span> / <span class=\"totalPages\"></span></div>"
        .to_string()
}

// ---------------------------------------------------------------------------
// Contents page: the sidebar tree
// ---------------------------------------------------------------------------

/// A node in the sidebar-shaped tree of pages.
enum Entry {
    /// Index into the page list.
    Page(usize),
    Dir(String, Vec<Entry>),
}

/// Arrange page paths the way the website's sidebar does: within each folder,
/// files first and then sub-folders, each sorted in natural order.
fn build_tree(paths: &[&str]) -> Vec<Entry> {
    /// One folder: its files first, then its sub-folders, each in natural order.
    fn level(items: &[(usize, &[&str])]) -> Vec<Entry> {
        let mut files: Vec<(usize, &str)> =
            items.iter().filter(|(_, segs)| segs.len() == 1).map(|(i, segs)| (*i, segs[0])).collect();
        files.sort_by(|a, b| natural_cmp(a.1, b.1));
        let mut out: Vec<Entry> = files.into_iter().map(|(i, _)| Entry::Page(i)).collect();

        let mut dirs: Vec<(&str, Vec<(usize, &[&str])>)> = Vec::new();
        for (i, segs) in items.iter().filter(|(_, segs)| segs.len() > 1) {
            match dirs.iter_mut().find(|(name, _)| *name == segs[0]) {
                Some((_, children)) => children.push((*i, &segs[1..])),
                None => dirs.push((segs[0], vec![(*i, &segs[1..])])),
            }
        }
        dirs.sort_by(|a, b| natural_cmp(a.0, b.0));
        for (name, children) in dirs {
            out.push(Entry::Dir(name.to_string(), level(&children)));
        }
        out
    }
    let split: Vec<Vec<&str>> = paths.iter().map(|p| p.split('/').collect()).collect();
    let items: Vec<(usize, &[&str])> = split.iter().enumerate().map(|(i, s)| (i, s.as_slice())).collect();
    level(&items)
}

/// Page indexes in reading order.
fn flatten(entries: &[Entry], out: &mut Vec<usize>) {
    for e in entries {
        match e {
            Entry::Page(i) => out.push(*i),
            Entry::Dir(_, children) => flatten(children, out),
        }
    }
}

/// Case-insensitive comparison that orders runs of digits by value, so
/// `page2` sorts before `page10`. This approximates the browser's collation
/// used by the sidebar.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(&c) = it.peek() {
                        if !c.is_ascii_digit() {
                            break;
                        }
                        s.push(c);
                        it.next();
                    }
                    s.trim_start_matches('0').to_string()
                };
                let (na, nb) = (run(&mut ai), run(&mut bi));
                let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(&nb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let (lx, ly) = (x.to_lowercase().next().unwrap_or(x), y.to_lowercase().next().unwrap_or(y));
                if lx != ly {
                    return lx.cmp(&ly);
                }
                ai.next();
                bi.next();
            }
        }
    }
}

/// The contents list: nested like the sidebar, each page a link to its section.
fn toc_html(tree: &[Entry], sections: &[Option<Section>], appendix: bool) -> String {
    /// Write one level of the contents list, recursing into folders.
    fn list(entries: &[Entry], sections: &[Option<Section>], out: &mut String) {
        out.push_str("<ul>\n");
        for e in entries {
            match e {
                Entry::Page(i) => {
                    if let Some(s) = &sections[*i] {
                        out.push_str(&format!("<li><a href=\"#{}\">{}</a></li>\n", esc(&s.id), esc(&s.title)));
                    }
                }
                Entry::Dir(name, children) => {
                    out.push_str(&format!("<li class=\"toc-dir\"><span>{}</span>\n", esc(name)));
                    list(children, sections, out);
                    out.push_str("</li>\n");
                }
            }
        }
        out.push_str("</ul>\n");
    }
    let mut out = String::new();
    list(tree, sections, &mut out);
    if appendix {
        out.push_str("<ul class=\"toc-appendix\"><li><a href=\"#appendix-todos\">Appendix: TODOs</a></li></ul>\n");
    }
    out
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// A `file://` URL for a local path (percent-encoded; Windows drive letters kept).
fn file_url(path: &Path) -> String {
    let raw = path.to_string_lossy().replace('\\', "/");
    let raw = raw.strip_prefix("//?/").unwrap_or(&raw); // Windows extended-length prefix
    let mut url = String::from("file://");
    if !raw.starts_with('/') {
        url.push('/'); // "C:/x" -> "/C:/x"
    }
    let encoded: Vec<String> = raw
        .split('/')
        .map(|seg| {
            let mut s = String::new();
            for b in seg.bytes() {
                if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b':') {
                    s.push(b as char);
                } else {
                    s.push_str(&format!("%{b:02X}"));
                }
            }
            s
        })
        .collect();
    url.push_str(&encoded.join("/"));
    url
}

/// Convert days since 1970-01-01 to a calendar date (proleptic Gregorian).
/// This is Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, 0..=146096
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Format seconds since the Unix epoch as `YYYY-MM-DD` (UTC).
fn format_date(epoch_secs: i64) -> String {
    let (y, m, d) = civil_from_days(epoch_secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today's date (UTC) for the cover. Honours `SOURCE_DATE_EPOCH`, the
/// convention for reproducible builds, so the same input gives the same PDF text.
fn today() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0));
    format_date(secs)
}

/// A temporary directory that is removed when dropped, even on an error path.
struct TempDir(PathBuf);

impl TempDir {
    /// Create a uniquely named directory under the system temp directory.
    fn new(prefix: &str) -> Result<TempDir> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir)?;
        // Canonical, so the staging copy is never mistaken for a path inside the project.
        Ok(TempDir(dir.canonicalize()?))
    }
    /// The directory's canonical path.
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_compares_numbers_by_value_and_ignores_case() {
        let mut v = vec!["page10.md", "Page2.md", "page1.md", "Advanced.md", "a.md"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a.md", "Advanced.md", "page1.md", "Page2.md", "page10.md"]);
        assert_eq!(natural_cmp("x007", "x7"), Ordering::Equal);
        assert_eq!(natural_cmp("a", "ab"), Ordering::Less);
    }

    #[test]
    fn tree_lists_files_before_folders_like_the_sidebar() {
        let paths = ["README.md", "docs/guide/setup.md", "docs/architecture.md", "TODO.md", "docs/guide/Advanced.md", "notes/n.md"];
        let tree = build_tree(&paths);
        let mut order = Vec::new();
        flatten(&tree, &mut order);
        let names: Vec<&str> = order.iter().map(|&i| paths[i]).collect();
        assert_eq!(
            names,
            vec!["README.md", "TODO.md", "docs/architecture.md", "docs/guide/Advanced.md", "docs/guide/setup.md", "notes/n.md"]
        );
        // every page appears exactly once
        assert_eq!(order.len(), paths.len());
    }

    #[test]
    fn dates_are_computed_correctly() {
        assert_eq!(format_date(0), "1970-01-01");
        assert_eq!(format_date(951_782_400), "2000-02-29"); // a leap day
        assert_eq!(format_date(1_700_000_000), "2023-11-14");
        assert_eq!(format_date(1_760_745_600), "2025-10-18");
        assert_eq!(format_date(-86_400), "1969-12-31");
    }

    #[test]
    fn file_urls_are_encoded() {
        assert_eq!(file_url(Path::new("/tmp/a b/print.html")), "file:///tmp/a%20b/print.html");
        assert_eq!(file_url(Path::new(r"C:\Users\me\x y\print.html")), "file:///C:/Users/me/x%20y/print.html");
        assert_eq!(file_url(Path::new(r"\\?\C:\t\p.html")), "file:///C:/t/p.html");
    }

    #[test]
    fn default_pdf_name_is_a_safe_slug() {
        assert_eq!(default_pdf_name("Acme Docs"), "acme-docs.pdf");
        assert_eq!(default_pdf_name("  --  "), "docs.pdf");
        assert_eq!(default_pdf_name("日本語"), "docs.pdf");
    }

    #[test]
    fn header_and_footer_escape_the_title_and_carry_page_numbers() {
        assert!(header_html("A <b> & C").contains("A &lt;b&gt; &amp; C"));
        let f = footer_html();
        assert!(f.contains("pageNumber") && f.contains("totalPages"));
    }

    /// Build a small project on disk and check the assembled document.
    #[test]
    fn print_document_has_cover_contents_pages_and_appendix() {
        let dir = std::env::temp_dir().join(format!("dokt-export-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("README.md"), "# Home\n\nSee [[docs/guide]] and [[guide#Install]].\n").unwrap();
        fs::write(dir.join("docs/guide.md"), "## Install\n\nTODO: fill this in\n\n![[pic.png|50]]\n").unwrap();
        fs::write(dir.join("TODO.md"), "- [ ] write more\n- [x] done thing\n").unwrap();
        fs::write(dir.join("pic.png"), b"x").unwrap();
        let root = dir.canonicalize().unwrap();
        let cfg: Config = toml::from_str("title = \"My Docs\"\n").unwrap();
        let crawled = crawl::crawl(&root, &cfg, &root.join("out")).unwrap();
        let doc = print_document(&root, &cfg, &crawled, Paper::Letter).unwrap();
        let html = &doc.html;

        assert_eq!(doc.pages, 3);
        assert!(html.contains("size: Letter"));
        assert!(html.contains("<h1 class=\"cover-title\">My Docs</h1>"));
        assert!(html.contains("3 pages"));
        // Contents are in sidebar order: files first (README, TODO), then the docs folder.
        let toc = html.split("<nav class=\"toc\"").nth(1).unwrap().split("</nav>").next().unwrap();
        let pos = |needle: &str| toc.find(needle).unwrap_or_else(|| panic!("{needle} missing from {toc}"));
        assert!(pos("Home") < pos(">TODO<") && pos(">TODO<") < pos("docs") && pos("docs") < pos(">guide<"));
        assert!(toc.contains("class=\"toc-dir\"") && toc.contains("Appendix: TODOs"));
        // A page without an H1 gets a title; one with an H1 does not get a second.
        assert!(html.contains("<h1 class=\"doc-title\">guide</h1>"));
        assert!(!html.contains("<h1 class=\"doc-title\">Home</h1>"));
        // Cross-page links are in-document, and heading ids are namespaced.
        assert!(html.contains("href=\"#p3-docs-guide\""), "{html}");
        assert!(html.contains("href=\"#p3-docs-guide--install\""));
        assert!(html.contains("id=\"p3-docs-guide--install\""));
        // Appendix lists TODOs from the page and from TODO.md, linking back by anchor.
        assert!(html.contains("id=\"appendix-todos\""));
        assert!(html.contains("<strong>2</strong> open, <strong>1</strong> done"), "{html}");
        assert!(html.contains("href=\"#p3-docs-guide--todo-1\""));
        // Images resolve from the document root and are not lazy.
        assert!(html.contains("src=\"pic.png\"") && !html.contains("loading=\"lazy\""));
        let _ = fs::remove_dir_all(&dir);
    }
}
