//! Markdown -> HTML, including Obsidian-style wikilink resolution, image
//! embeds, and Mermaid block handling.

use crate::config::Todos;
use crate::todo::{self, TodoEntry};
use comrak::nodes::{NodeLink, NodeValue};
use comrak::{format_html, parse_document, Anchorizer, Arena, Options};
use std::collections::HashMap;
use std::path::Path;

/// Image types that can be embedded in pages.
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "bmp", "ico"];
/// Other files that pages may link to and that are copied into the site.
const OTHER_ASSET_EXTS: &[&str] = &["pdf"];

/// Lowercased extension of a path or file name.
fn ext_of(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())
}

/// Accepts a bare extension (`png`) or anything with one (`a/b.png`).
pub fn is_asset(path_or_ext: &str) -> bool {
    let ext = if path_or_ext.contains('.') { ext_of(path_or_ext) } else { Some(path_or_ext.to_ascii_lowercase()) };
    ext.map_or(false, |e| IMAGE_EXTS.contains(&e.as_str()) || OTHER_ASSET_EXTS.contains(&e.as_str()))
}

/// True if the path has an image extension.
fn is_image(path: &str) -> bool {
    ext_of(path).map_or(false, |e| IMAGE_EXTS.contains(&e.as_str()))
}

/// A page, as far as link resolution is concerned.
pub struct PageInfo {
    /// Source path relative to the project root, e.g. `docs/guide/setup.md`.
    pub rel: String,
    /// Output path relative to the site root, e.g. `docs/guide/setup.html`.
    pub html: String,
    /// File name without extension, e.g. `setup`.
    pub stem: String,
    /// Unique element id for this page when all pages share one document (PDF export).
    pub id: String,
}

/// The outcome of looking up a link target.
pub enum Resolved {
    Found(usize),
    /// Several files matched; the best guess and how many matched.
    Ambiguous(usize, usize),
    Missing,
}

/// Case-insensitive lookup of files by path, partial path, or bare name.
struct Lookup {
    rels: Vec<String>,
    keys: Vec<String>,
    by_key: HashMap<String, usize>,
    by_name: HashMap<String, Vec<usize>>,
    strip_ext: bool,
}

/// The lookup key for a path: slashes normalised, surrounding slashes removed,
/// lowercased, and (for pages) without the `.md` extension.
fn key_of(s: &str, strip_ext: bool) -> String {
    let t = s.replace('\\', "/");
    let t = t.trim().trim_matches('/');
    let t = if strip_ext { strip_md_ext(t) } else { t };
    t.to_lowercase()
}

impl Lookup {
    /// Index `rels` (project-relative paths). With `strip_ext`, keys omit the `.md`
    /// extension, which is how page links are written; assets keep theirs.
    fn new(rels: Vec<String>, strip_ext: bool) -> Self {
        let keys: Vec<String> = rels.iter().map(|r| key_of(r, strip_ext)).collect();
        let mut by_key = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, k) in keys.iter().enumerate() {
            by_key.insert(k.clone(), i);
            by_name.entry(k.rsplit('/').next().unwrap_or(k).to_string()).or_default().push(i);
        }
        Lookup { rels, keys, by_key, by_name, strip_ext }
    }

    /// Exact lookup by project-relative path.
    fn find(&self, rel: &str) -> Option<usize> {
        self.by_key.get(&key_of(rel, self.strip_ext)).copied()
    }

    /// Obsidian-style resolution:
    /// 1. a path from the project root (`docs/guide/setup`),
    /// 2. a trailing part of a path (`guide/setup`),
    /// 3. a bare file name (`setup`).
    /// `./` and `../` are resolved relative to the linking page.
    fn resolve(&self, target: &str, from_dir: &str) -> Resolved {
        let t = target.trim().replace('\\', "/");
        if t.starts_with("./") || t.starts_with("../") {
            let joined = if from_dir.is_empty() { t.clone() } else { format!("{from_dir}/{t}") };
            return match normalize(&joined).and_then(|n| self.find(&n)) {
                Some(i) => Resolved::Found(i),
                None => Resolved::Missing,
            };
        }
        let key = key_of(&t, self.strip_ext);
        if key.is_empty() {
            return Resolved::Missing;
        }
        if let Some(&i) = self.by_key.get(&key) {
            return Resolved::Found(i);
        }
        let candidates: Vec<usize> = if key.contains('/') {
            let suffix = format!("/{key}");
            (0..self.rels.len()).filter(|&i| format!("/{}", self.keys[i]).ends_with(&suffix)).collect()
        } else {
            self.by_name.get(&key).cloned().unwrap_or_default()
        };
        match candidates.len() {
            0 => Resolved::Missing,
            1 => Resolved::Found(candidates[0]),
            n => {
                let best = candidates
                    .iter()
                    .copied()
                    .min_by_key(|&i| (dir_of(&self.rels[i]) != from_dir, self.rels[i].len()))
                    .unwrap();
                Resolved::Ambiguous(best, n)
            }
        }
    }
}

/// Lookup tables over every page and asset, used to resolve links.
pub struct Index {
    pub pages: Vec<PageInfo>,
    page_lookup: Lookup,
    asset_lookup: Lookup,
}

impl Index {
    /// `assets` are project-relative paths of every image and PDF that was crawled.
    pub fn new(pages: Vec<PageInfo>, assets: Vec<String>) -> Self {
        let page_lookup = Lookup::new(pages.iter().map(|p| p.rel.clone()).collect(), true);
        Index { pages, page_lookup, asset_lookup: Lookup::new(assets, false) }
    }

    /// Resolve a wikilink target to a page. See [`Lookup::resolve`] for the rules.
    pub fn resolve(&self, target: &str, from_dir: &str) -> Resolved {
        self.page_lookup.resolve(target, from_dir)
    }

    /// Exact lookup of a page by its project-relative path.
    pub fn find_rel(&self, rel: &str) -> Option<usize> {
        self.page_lookup.find(rel)
    }

    /// Resolve a wikilink target to an image or PDF. The extension is required.
    fn resolve_asset(&self, target: &str, from_dir: &str) -> Resolved {
        self.asset_lookup.resolve(target, from_dir)
    }

    /// The project-relative path of asset number `i`.
    fn asset_rel(&self, i: usize) -> &str {
        &self.asset_lookup.rels[i]
    }
}

/// What the rendered HTML is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// One HTML file per page. Links between pages are relative file URLs.
    Site,
    /// All pages in a single document (the PDF). Links between pages become
    /// in-document anchors, and ids are prefixed per page so they cannot clash.
    Print,
}

/// Everything `render` needs to know about the surrounding project.
pub struct Ctx<'a> {
    pub index: &'a Index,
    /// Canonical project root; referenced files must live inside it.
    pub root: &'a Path,
    pub allow_html: bool,
    pub todos: &'a Todos,
    pub target: Target,
}

/// One rendered page.
pub struct Rendered {
    pub title: Option<String>,
    pub html: String,
    pub warnings: Vec<String>,
    pub todos: Vec<TodoEntry>,
    /// Whether the page has a level-1 heading of its own (the PDF adds a title if not).
    pub has_h1: bool,
    /// Project-relative paths of images/PDFs this page uses. The site builder
    /// copies them, even if the `include` globs did not match them.
    pub assets: Vec<String>,
}

/// Render one page of Markdown to an HTML fragment.
///
/// The document is parsed once and then edited in place before it is turned into
/// HTML. In order:
///
/// 1. **Links and embeds** are resolved: `[[wikilinks]]` (pages, images, PDFs),
///    `![[image]]` embeds, ordinary `[links](x.md)` and `![images](x.png)`.
/// 2. **Mermaid** code blocks become `<pre class="mermaid">` for the browser to draw.
/// 3. **TODOs** are highlighted and collected (see [`crate::todo`]).
///
/// `page_html` is where the output will live, which decides how relative URLs are
/// written. `ctx.target` chooses between the website and the single-document PDF.
pub fn render(source: &str, page_rel: &str, page_html: &str, ctx: &Ctx) -> Rendered {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.extension.alerts = true;
    options.extension.front_matter_delimiter = Some("---".to_string());
    // In a single-document export every page needs its own heading id namespace.
    let page_id = ctx.index.find_rel(page_rel).map(|i| ctx.index.pages[i].id.clone()).unwrap_or_default();
    options.extension.header_id_prefix = Some(match ctx.target {
        Target::Site => String::new(),
        Target::Print => format!("{page_id}--"),
    });
    options.extension.wikilinks_title_after_pipe = true; // [[target|alias]]
    options.render.r#unsafe = ctx.allow_html;

    let arena = Arena::new();
    let root = parse_document(&arena, source, &options);

    let from_dir = dir_of(page_rel).to_string();
    let mut warnings = Vec::new();
    let mut assets: Vec<String> = Vec::new();
    let mut fm_title: Option<String> = None;
    let mut h1_title: Option<String> = None;
    let mut has_h1 = false;

    /// What to do with a node, decided up front so the node's data is not borrowed
    /// while it is being changed.
    enum Action {
        Wiki(String),
        Link(String),
        Image(String),
        Embeds,
        Mermaid(String),
        Heading1,
        Front(String),
    }

    let nodes: Vec<_> = root.descendants().collect();
    for node in nodes {
        let action = match &node.data().value {
            NodeValue::WikiLink(w) => Action::Wiki(w.url.clone()),
            NodeValue::Link(l) => Action::Link(l.url.clone()),
            NodeValue::Image(l) => Action::Image(l.url.clone()),
            NodeValue::Text(t) if t.contains("![[") => Action::Embeds,
            NodeValue::CodeBlock(c) if is_mermaid(&c.info) => Action::Mermaid(c.literal.clone()),
            NodeValue::Heading(h) if h.level == 1 && h1_title.is_none() => Action::Heading1,
            NodeValue::FrontMatter(f) => Action::Front(f.clone()),
            _ => continue,
        };

        match action {
            Action::Front(text) => fm_title = front_matter_title(&text),
            Action::Heading1 => {
                has_h1 = true;
                let title = plain_text(node);
                if !title.is_empty() {
                    h1_title = Some(title);
                }
            }
            Action::Mermaid(code) => {
                node.data_mut().value =
                    NodeValue::Raw(format!("<pre class=\"mermaid\">{}</pre>\n", esc(&code)));
            }
            Action::Link(url) => {
                if let Some(new_url) =
                    rewrite_link(&url, &from_dir, page_html, ctx, &mut warnings, &mut assets, page_rel)
                {
                    if let NodeValue::Link(l) = &mut node.data_mut().value {
                        l.url = new_url;
                    }
                }
            }
            Action::Image(url) => {
                if is_external(&url) {
                    continue;
                }
                let alt_raw = plain_text(node);
                let (alt, size) = split_alt_size(&alt_raw);
                let title = match &node.data().value {
                    NodeValue::Image(l) => l.title.clone(),
                    _ => String::new(),
                };
                match locate_file(&url, &from_dir, ctx) {
                    Some(rel) => {
                        let src = rel_url(page_html, &rel);
                        assets.push(rel);
                        if size.is_some() {
                            // comrak cannot set width/height, so emit the tag ourselves.
                            let tag = img_tag(&src, alt, size, Some(&title));
                            for child in node.children().collect::<Vec<_>>() {
                                child.detach();
                            }
                            node.data_mut().value = NodeValue::Raw(tag);
                        } else if let NodeValue::Image(l) = &mut node.data_mut().value {
                            l.url = src;
                        }
                    }
                    None => warnings.push(format!("{page_rel}: image {url} not found")),
                }
            }
            Action::Embeds => {
                let text = match &node.data().value {
                    NodeValue::Text(t) => t.to_string(),
                    _ => continue,
                };
                for (is_embed, piece) in split_embeds(&text) {
                    let value = if is_embed {
                        NodeValue::Raw(embed_html(piece, page_rel, page_html, &page_id, &from_dir, ctx, &mut warnings, &mut assets))
                    } else {
                        NodeValue::Text(piece.to_string().into())
                    };
                    node.insert_before(arena.alloc(value.into()));
                }
                node.detach();
            }
            Action::Wiki(url) => {
                let (target, anchor) = match url.split_once('#') {
                    Some((t, a)) => (t.to_string(), Some(a.trim().to_string())),
                    None => (url.clone(), None),
                };
                // Obsidian block references (`#^id`) just link to the page.
                let anchor = anchor.filter(|a| !a.is_empty() && !a.starts_with('^'));
                let slug = anchor.as_ref().map(|a| Anchorizer::new().anchorize(a)).unwrap_or_default();

                let (href, name, found) =
                    resolve_target(&target, &from_dir, page_html, page_rel, ctx, &mut warnings);

                // Show only the file name unless the author wrote an alias.
                if let Some(child) = node.first_child() {
                    if let NodeValue::Text(t) = &mut child.data_mut().value {
                        if t.to_string() == url {
                            let shown = match (&anchor, is_asset(&target)) {
                                (Some(a), false) => format!("{name} > {a}"),
                                _ => name.clone(),
                            };
                            *t = shown.into();
                        }
                    }
                }

                if found {
                    // Images and PDFs have no headings to jump to.
                    let slug = if is_asset(&target) { String::new() } else { slug };
                    let new_url = join_anchor(ctx.target, &href, &page_id, &slug);
                    if let NodeValue::WikiLink(w) = &mut node.data_mut().value {
                        w.url = new_url;
                    }
                } else {
                    node.data_mut().value = NodeValue::Link(Box::new(NodeLink {
                        url: "#".to_string(),
                        title: format!("Not found: {target}"),
                    }));
                }
            }
        }
    }

    // Runs after link rewriting (so link labels are already final) and after
    // the title has been read from the headings.
    let todos = if ctx.todos.enabled {
        // Ids must be unique across the whole document in a PDF.
        let prefix = match ctx.target {
            Target::Site => "dokt-".to_string(),
            Target::Print => format!("{page_id}--"),
        };
        todo::process(&arena, root, ctx.todos, todo::is_todo_file(page_rel, ctx.todos), &prefix)
    } else {
        Vec::new()
    };

    let mut html = String::new();
    format_html(root, &options, &mut html).expect("writing to a String cannot fail");
    // Let the browser defer off-screen images (covers comrak's own tags and ours).
    // Not in a PDF: the browser must have every image loaded before it prints.
    let html = match ctx.target {
        Target::Site => html.replace("<img src=\"", "<img loading=\"lazy\" decoding=\"async\" src=\""),
        Target::Print => html,
    };

    assets.sort();
    assets.dedup();
    Rendered { title: fm_title.or(h1_title), html, warnings, todos, has_h1, assets }
}

// ---------- link and embed resolution ----------

/// Resolve a wikilink target to `(href, display name, found)`. Handles pages,
/// and images/PDFs (when the target has an asset extension).
fn resolve_target(
    target: &str,
    from_dir: &str,
    page_html: &str,
    page_rel: &str,
    ctx: &Ctx,
    warnings: &mut Vec<String>,
) -> (String, String, bool) {
    let target = target.trim();
    if target.is_empty() {
        return (String::new(), stem_of(page_rel).to_string(), true);
    }
    if is_asset(target) {
        return match resolve_asset_target(target, from_dir, page_rel, ctx, warnings) {
            Some(rel) => (rel_url(page_html, &rel), file_name(&rel).to_string(), true),
            None => {
                warnings.push(format!("{page_rel}: [[{target}]] not found"));
                (String::new(), file_name(target).to_string(), false)
            }
        };
    }
    match ctx.index.resolve(target, from_dir) {
        Resolved::Found(i) => (page_href(ctx, page_html, i), ctx.index.pages[i].stem.clone(), true),
        Resolved::Ambiguous(i, n) => {
            warnings.push(format!("{page_rel}: [[{target}]] matches {n} pages; using {}", ctx.index.pages[i].rel));
            (page_href(ctx, page_html, i), ctx.index.pages[i].stem.clone(), true)
        }
        Resolved::Missing => {
            warnings.push(format!("{page_rel}: [[{target}]] not found"));
            (String::new(), file_name(target).to_string(), false)
        }
    }
}

/// Find an image/PDF for a wikilink: by name or path among the crawled
/// assets first, then as a file path relative to the page or the project root.
fn resolve_asset_target(
    target: &str,
    from_dir: &str,
    page_rel: &str,
    ctx: &Ctx,
    warnings: &mut Vec<String>,
) -> Option<String> {
    match ctx.index.resolve_asset(target, from_dir) {
        Resolved::Found(i) => return Some(ctx.index.asset_rel(i).to_string()),
        Resolved::Ambiguous(i, n) => {
            warnings.push(format!("{page_rel}: [[{target}]] matches {n} files; using {}", ctx.index.asset_rel(i)));
            return Some(ctx.index.asset_rel(i).to_string());
        }
        Resolved::Missing => {}
    }
    locate_file(target, from_dir, ctx).or_else(|| {
        let rel = normalize(target.trim_start_matches('/'))?;
        file_in_root(&rel, ctx.root).then_some(rel)
    })
}

/// A file referenced by a standard Markdown URL: relative to the page, or to
/// the project root when it starts with `/`. Must exist inside the project.
fn locate_file(url: &str, from_dir: &str, ctx: &Ctx) -> Option<String> {
    let end = url.find(['#', '?']).unwrap_or(url.len());
    let path = percent_decode(&url[..end]);
    if path.is_empty() || !is_asset(&path) {
        return None;
    }
    let joined = if let Some(r) = path.strip_prefix('/') {
        r.to_string()
    } else if from_dir.is_empty() {
        path.clone()
    } else {
        format!("{from_dir}/{path}")
    };
    let rel = normalize(&joined)?;
    file_in_root(&rel, ctx.root).then_some(rel)
}

/// True if `rel` is a regular file inside `root` (symlinks may not escape it).
fn file_in_root(rel: &str, root: &Path) -> bool {
    match root.join(rel).canonicalize() {
        Ok(c) => c.is_file() && c.starts_with(root),
        Err(_) => false,
    }
}

/// Split text into literal pieces and `![[...]]` embeds: `(is_embed, text)`.
fn split_embeds(text: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(rel) = text[i..].find("![[") {
        let start = i + rel;
        let inner_start = start + 3;
        let Some(len) = text[inner_start..].find("]]") else { break };
        let inner = &text[inner_start..inner_start + len];
        if inner.trim().is_empty() || inner.contains(['\n', '[', ']']) {
            out.push((false, &text[i..inner_start]));
            i = inner_start;
            continue;
        }
        if start > i {
            out.push((false, &text[i..start]));
        }
        out.push((true, inner));
        i = inner_start + len + 2;
    }
    if i < text.len() {
        out.push((false, &text[i..]));
    }
    out
}

/// `300` or `300x200`.
fn parse_size(s: &str) -> Option<(u32, Option<u32>)> {
    let (w, h) = match s.split_once(['x', 'X']) {
        Some((w, h)) => (w, Some(h)),
        None => (s, None),
    };
    let w: u32 = w.parse().ok()?;
    let h = match h {
        Some(h) => Some(h.parse::<u32>().ok()?),
        None => None,
    };
    (w > 0 && w <= 10_000 && h.map_or(true, |h| h > 0 && h <= 10_000)).then_some((w, h))
}

/// `my alt|300` -> (`my alt`, Some(300)).
fn split_alt_size(alt: &str) -> (&str, Option<(u32, Option<u32>)>) {
    match alt.rsplit_once('|') {
        Some((a, s)) => match parse_size(s.trim()) {
            Some(size) => (a.trim(), Some(size)),
            None => (alt, None),
        },
        None => (alt, None),
    }
}

/// Build an `<img>`. The browser reads `width`/`height`, and `loading=lazy` is added later.
fn img_tag(src: &str, alt: &str, size: Option<(u32, Option<u32>)>, title: Option<&str>) -> String {
    let mut s = format!("<img src=\"{}\" alt=\"{}\"", esc(src), esc(alt));
    if let Some((w, h)) = size {
        s.push_str(&format!(" width=\"{w}\""));
        if let Some(h) = h {
            s.push_str(&format!(" height=\"{h}\""));
        }
    }
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        s.push_str(&format!(" title=\"{}\"", esc(t)));
    }
    s.push_str(" />");
    s
}

/// HTML for one `![[inner]]`: an image, or (with a warning) a link for
/// anything that is not an image, because embedding notes is not supported.
fn embed_html(
    inner: &str,
    page_rel: &str,
    page_html: &str,
    page_id: &str,
    from_dir: &str,
    ctx: &Ctx,
    warnings: &mut Vec<String>,
    assets: &mut Vec<String>,
) -> String {
    let mut parts = inner.split('|');
    let target_full = parts.next().unwrap_or("").trim();
    let (target, anchor) = match target_full.split_once('#') {
        Some((t, a)) => (t.trim(), Some(a.trim())),
        None => (target_full, None),
    };
    let mut size = None;
    let mut alt: Option<String> = None;
    for p in parts {
        let p = p.trim();
        if let Some(s) = parse_size(p) {
            size = Some(s);
        } else if alt.is_none() && !p.is_empty() {
            alt = Some(p.to_string());
        }
    }

    if is_image(target) {
        return match resolve_asset_target(target, from_dir, page_rel, ctx, warnings) {
            Some(rel) => {
                let src = rel_url(page_html, &rel);
                let alt = alt.unwrap_or_else(|| strip_ext(file_name(&rel)).to_string());
                assets.push(rel);
                img_tag(&src, &alt, size, None)
            }
            None => {
                warnings.push(format!("{page_rel}: image ![[{target}]] not found"));
                format!(
                    "<span class=\"broken-embed\" title=\"Image not found: {t}\">{t}</span>",
                    t = esc(target)
                )
            }
        };
    }

    // Not an image: link to it instead.
    warnings.push(format!("{page_rel}: ![[{target}]] is not an image; embedding notes is not supported, linked instead"));
    let (href, name, found) = resolve_target(target, from_dir, page_html, page_rel, ctx, warnings);
    if !found {
        return format!("<a href=\"#\" title=\"Not found: {t}\">{n}</a>", t = esc(target), n = esc(&name));
    }
    let slug = match anchor {
        Some(a) if !a.is_empty() && !a.starts_with('^') && !is_asset(target) => Anchorizer::new().anchorize(a),
        _ => String::new(),
    };
    let href = join_anchor(ctx.target, &href, page_id, &slug);
    format!("<a href=\"{}\" data-wikilink=\"true\">{}</a>", esc(&href), esc(&name))
}

/// Rewrite an ordinary Markdown link: `.md` links point at the generated page,
/// and links to images and PDFs make sure the file ships with the site.
/// Returns `None` to leave the link as it is.
fn rewrite_link(
    url: &str,
    from_dir: &str,
    page_html: &str,
    ctx: &Ctx,
    warnings: &mut Vec<String>,
    assets: &mut Vec<String>,
    page_rel: &str,
) -> Option<String> {
    if is_external(url) || url.is_empty() || url.starts_with('#') {
        return None;
    }
    let split = url.find(['#', '?']).unwrap_or(url.len());
    let (path_part, rest) = url.split_at(split);
    let path = percent_decode(path_part);
    let lp = path.to_ascii_lowercase();

    // Links to images and PDFs: make sure the file ships with the site.
    if is_asset(&path) {
        return match locate_file(url, from_dir, ctx) {
            Some(rel) => {
                let new_url = format!("{}{}", rel_url(page_html, &rel), rest);
                assets.push(rel);
                Some(new_url)
            }
            None => {
                warnings.push(format!("{page_rel}: linked file {path} not found"));
                None
            }
        };
    }

    if !(lp.ends_with(".md") || lp.ends_with(".markdown")) {
        return None;
    }
    let joined = if let Some(stripped) = path.strip_prefix('/') {
        stripped.to_string()
    } else if from_dir.is_empty() {
        path.clone()
    } else {
        format!("{from_dir}/{path}")
    };
    match normalize(&joined).and_then(|n| ctx.index.find_rel(&n)) {
        Some(i) => Some(match ctx.target {
            Target::Site => format!("{}{}", rel_url(page_html, &ctx.index.pages[i].html), rest),
            // `rest` is "#heading" and/or "?query"; only the heading survives in a single document.
            Target::Print => join_anchor(
                Target::Print,
                &page_href(ctx, page_html, i),
                "",
                rest.strip_prefix('#').map(|f| f.split('?').next().unwrap_or("")).unwrap_or(""),
            ),
        }),
        None => {
            warnings.push(format!("{page_rel}: link to {path} points outside the included docs"));
            None
        }
    }
}

// ---------- small helpers ----------

/// Where a link to page `i` points: a relative file URL on the site, or the
/// page's section in a single-document export.
fn page_href(ctx: &Ctx, from_html: &str, i: usize) -> String {
    match ctx.target {
        Target::Site => rel_url(from_html, &ctx.index.pages[i].html),
        Target::Print => format!("#{}", ctx.index.pages[i].id),
    }
}

/// Attach a heading slug to a page link. An empty `href` means "this page".
/// On the site that is `page.html#slug`; in a single document, headings are
/// namespaced per page, so the target is `#<page id>--<slug>`.
fn join_anchor(target: Target, href: &str, self_id: &str, slug: &str) -> String {
    match target {
        Target::Site => match (href.is_empty(), slug.is_empty()) {
            (true, true) => "#".to_string(),
            (true, false) => format!("#{slug}"),
            (false, true) => href.to_string(),
            (false, false) => format!("{href}#{slug}"),
        },
        Target::Print => {
            let base = if href.is_empty() { format!("#{self_id}") } else { href.to_string() };
            if slug.is_empty() { base } else { format!("{base}--{slug}") }
        }
    }
}

/// True for URLs that point outside the project (`https://`, `mailto:`, `data:` ...).
fn is_external(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    url.starts_with("//")
        || url.contains("://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
        || lower.starts_with("data:")
}

/// Visible text of a node's descendants.
fn plain_text<'a>(node: &'a comrak::nodes::AstNode<'a>) -> String {
    let mut s = String::new();
    for d in node.descendants().skip(1) {
        match &d.data().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(c) => s.push_str(&c.literal),
            _ => {}
        }
    }
    s.trim().to_string()
}

/// True for a fenced code block tagged `mermaid`.
fn is_mermaid(info: &str) -> bool {
    info.split_whitespace().next().map(|w| w.eq_ignore_ascii_case("mermaid")).unwrap_or(false)
}

/// Read `title:` from front matter. Only that one key is understood.
fn front_matter_title(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(v) = line.trim().strip_prefix("title:") {
            let v = v.trim().trim_matches(|c| c == '"' || c == '\'').trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Remove a `.md` or `.markdown` extension.
pub fn strip_md_ext(s: &str) -> &str {
    let l = s.to_ascii_lowercase();
    if l.ends_with(".markdown") {
        &s[..s.len() - 9]
    } else if l.ends_with(".md") {
        &s[..s.len() - 3]
    } else {
        s
    }
}

/// Remove the last extension, if any.
fn strip_ext(s: &str) -> &str {
    s.rsplit_once('.').map(|(a, _)| a).filter(|a| !a.is_empty()).unwrap_or(s)
}

/// The directory part of a `/`-separated path (empty for a top-level file).
pub fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

/// The last component of a `/`-separated path.
pub fn file_name(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(_, f)| f).unwrap_or(rel)
}

/// A page's file name without its extension.
pub fn stem_of(rel: &str) -> &str {
    strip_md_ext(file_name(rel))
}

/// Resolve `.` and `..`; `None` if the path escapes the root.
fn normalize(path: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            p => out.push(p),
        }
    }
    Some(out.join("/"))
}

/// Relative URL from one output file to another, percent-encoded.
pub fn rel_url(from_html: &str, to_html: &str) -> String {
    let from: Vec<&str> = from_html.split('/').collect();
    let to: Vec<&str> = to_html.split('/').collect();
    let from_dirs = &from[..from.len() - 1];
    let to_dirs = &to[..to.len() - 1];
    let common = from_dirs.iter().zip(to_dirs.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from_dirs.len() - common];
    parts.extend(to[common..].iter().map(|s| encode_segment(s)));
    parts.join("/")
}

/// "../" repeated once per directory level of an output path.
pub fn root_prefix(html_path: &str) -> String {
    "../".repeat(html_path.matches('/').count())
}

/// Percent-encode one URL path segment.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decode `%XX` escapes. Malformed escapes are left as they are.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Escape text for use in HTML, including inside a quoted attribute.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn index(paths: &[&str]) -> Index {
        index_with(paths, &[])
    }

    fn index_with(paths: &[&str], assets: &[&str]) -> Index {
        Index::new(
            paths
                .iter()
                .enumerate()
                .map(|(i, p)| PageInfo {
                    rel: p.to_string(),
                    html: format!("{}.html", strip_md_ext(p)),
                    stem: stem_of(p).to_string(),
                    id: format!("p{}", i + 1),
                })
                .collect(),
            assets.iter().map(|a| a.to_string()).collect(),
        )
    }

    fn found(r: Resolved) -> Option<usize> {
        match r {
            Resolved::Found(i) | Resolved::Ambiguous(i, _) => Some(i),
            Resolved::Missing => None,
        }
    }

    #[test]
    fn resolves_full_path_suffix_and_bare_name() {
        let idx = index(&["README.md", "docs/guide/setup.md", "docs/api/overview.md"]);
        assert_eq!(found(idx.resolve("docs/guide/setup", "")), Some(1));
        assert_eq!(found(idx.resolve("docs/guide/setup.md", "")), Some(1));
        assert_eq!(found(idx.resolve("guide/setup", "")), Some(1));
        assert_eq!(found(idx.resolve("setup", "")), Some(1));
        assert_eq!(found(idx.resolve("SETUP", "")), Some(1)); // case-insensitive
        assert_eq!(found(idx.resolve("nope", "")), None);
        assert_eq!(found(idx.resolve("", "")), None);
    }

    #[test]
    fn resolves_relative_links_from_the_linking_page() {
        let idx = index(&["docs/guide/setup.md", "docs/guide/advanced.md", "docs/architecture.md"]);
        assert_eq!(found(idx.resolve("./advanced", "docs/guide")), Some(1));
        assert_eq!(found(idx.resolve("../architecture", "docs/guide")), Some(2));
        assert_eq!(found(idx.resolve("../../../escape", "docs/guide")), None);
    }

    #[test]
    fn ambiguous_names_prefer_the_same_directory_then_shortest_path() {
        let idx = index(&["a/overview.md", "b/c/overview.md", "b/overview.md"]);
        match idx.resolve("overview", "b/c") {
            Resolved::Ambiguous(i, 3) => assert_eq!(i, 1), // same directory wins
            _ => panic!("expected ambiguity"),
        }
        match idx.resolve("overview", "") {
            Resolved::Ambiguous(i, 3) => assert_eq!(i, 0), // shortest path
            _ => panic!("expected ambiguity"),
        }
        assert_eq!(found(idx.resolve("b/overview", "")), Some(2));
    }

    #[test]
    fn asset_lookup_keeps_the_extension() {
        let idx = index_with(&[], &["docs/img/Logo.PNG", "other/logo.png", "docs/shot.png"]);
        assert_eq!(found(idx.resolve_asset("shot.png", "")), Some(2));
        assert_eq!(found(idx.resolve_asset("docs/img/logo.png", "")), Some(0));
        assert_eq!(found(idx.resolve_asset("shot", "")), None); // extension is required
        match idx.resolve_asset("logo.png", "other") {
            Resolved::Ambiguous(i, 2) => assert_eq!(i, 1),
            _ => panic!("expected ambiguity"),
        }
    }

    #[test]
    fn relative_urls_between_output_files() {
        assert_eq!(rel_url("README.html", "docs/a.html"), "docs/a.html");
        assert_eq!(rel_url("docs/a.html", "README.html"), "../README.html");
        assert_eq!(rel_url("docs/a.html", "docs/b.html"), "b.html");
        assert_eq!(rel_url("docs/x/a.html", "docs/y/b.html"), "../y/b.html");
        assert_eq!(rel_url("a.html", "My Notes/x y.html"), "My%20Notes/x%20y.html");
        assert_eq!(root_prefix("a/b/c.html"), "../../");
        assert_eq!(root_prefix("index.html"), "");
    }

    #[test]
    fn percent_decoding_is_safe_on_odd_input() {
        assert_eq!(percent_decode("My%20Notes.md"), "My Notes.md");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%é"), "%zz%é");
        assert_eq!(percent_decode("%e2%9c%93"), "✓");
    }

    /// A throwaway project directory with some files in it.
    fn project(files: &[&str]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("dokt-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = fs::remove_dir_all(&dir);
        for f in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    fn run(root: &Path, idx: &Index, rel: &str, src: &str) -> Rendered {
        let todos = Todos::default();
        let ctx = Ctx { index: idx, root, allow_html: true, todos: &todos, target: Target::Site };
        render(src, rel, &format!("{}.html", strip_md_ext(rel)), &ctx)
    }

    /// Like `run`, but for the single-document (PDF) target.
    fn run_print(root: &Path, idx: &Index, rel: &str, src: &str) -> Rendered {
        let todos = Todos::default();
        let ctx = Ctx { index: idx, root, allow_html: true, todos: &todos, target: Target::Print };
        render(src, rel, "print.html", &ctx)
    }

    #[test]
    fn wikilinks_show_the_file_name_and_mermaid_becomes_a_pre() {
        let root = project(&[]);
        let idx = index(&["README.md", "docs/guide/setup.md"]);
        let src = "[[docs/guide/setup]] [[docs/guide/setup|the setup]] [[setup#Install Steps]] [[gone]]\n\n```mermaid\ngraph TD; A-->B\n```\n";
        let r = run(&root, &idx, "README.md", src);
        assert!(r.html.contains(r#"<a href="docs/guide/setup.html" data-wikilink="true">setup</a>"#), "{}", r.html);
        assert!(r.html.contains(">the setup</a>"));
        assert!(r.html.contains(r#"href="docs/guide/setup.html#install-steps""#));
        assert!(r.html.contains("setup &gt; Install Steps"));
        assert!(r.html.contains(r#"title="Not found: gone""#));
        assert!(r.html.contains(r#"<pre class="mermaid">graph TD; A--&gt;B"#));
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn mermaid_survives_with_raw_html_disabled() {
        let root = project(&[]);
        let idx = index(&["a.md"]);
        let todos = Todos::default();
        let ctx = Ctx { index: &idx, root: &root, allow_html: false, todos: &todos, target: Target::Site };
        let r = render("```mermaid\ngraph TD; A-->B\n```\n\n<script>x</script>", "a.md", "a.html", &ctx);
        assert!(r.html.contains(r#"<pre class="mermaid">"#));
        assert!(!r.html.contains("<script>"));
    }

    #[test]
    fn embeds_find_images_by_name_path_and_size() {
        let root = project(&["docs/img/shot.png", "assets/logo.svg"]);
        let idx = index_with(&["docs/guide/a.md"], &["docs/img/shot.png", "assets/logo.svg"]);
        let src = "![[shot.png]] ![[docs/img/shot.png|300]] ![[shot.png|300x200]] ![[logo.svg|Our logo]] ![[logo.svg|Our logo|64]]";
        let r = run(&root, &idx, "docs/guide/a.md", src);
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(r.html.contains(r#"<img loading="lazy" decoding="async" src="../img/shot.png" alt="shot" />"#), "{}", r.html);
        assert!(r.html.contains(r#"src="../img/shot.png" alt="shot" width="300" />"#));
        assert!(r.html.contains(r#"width="300" height="200" />"#));
        assert!(r.html.contains(r#"src="../../assets/logo.svg" alt="Our logo" />"#));
        assert!(r.html.contains(r#"alt="Our logo" width="64" />"#));
        assert_eq!(r.assets, vec!["assets/logo.svg", "docs/img/shot.png"]);
    }

    #[test]
    fn embeds_use_files_outside_the_crawled_set_when_given_a_path() {
        let root = project(&["media/pic one.png"]);
        let idx = index(&["a.md"]); // the image was NOT crawled
        let r = run(&root, &idx, "a.md", "![[media/pic one.png]]");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(r.html.contains(r#"src="media/pic%20one.png""#), "{}", r.html);
        assert_eq!(r.assets, vec!["media/pic one.png"]);
        // ...but a bare name cannot be searched for on disk.
        let r = run(&root, &idx, "a.md", "![[pic one.png]]");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.html.contains("broken-embed"));
    }

    #[test]
    fn embeds_are_ignored_in_code_and_odd_syntax_stays_literal() {
        let root = project(&[]);
        let idx = index(&["a.md"]);
        let r = run(&root, &idx, "a.md", "`![[x.png]]`\n\n```\n![[x.png]]\n```\n\n![[]] and ![[ a\nb ]]\n");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(!r.html.contains("<img") && !r.html.contains("broken-embed"));
    }

    #[test]
    fn embedding_a_note_links_to_it_with_a_warning() {
        let root = project(&[]);
        let idx = index(&["a.md", "docs/other.md"]);
        let r = run(&root, &idx, "a.md", "before ![[other#Part One]] after");
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("not an image"));
        assert!(r.html.contains(r#"<a href="docs/other.html#part-one" data-wikilink="true">other</a>"#), "{}", r.html);
    }

    #[test]
    fn plain_wikilinks_can_point_at_images_and_pdfs() {
        let root = project(&[]);
        let idx = index_with(&["a.md"], &["files/spec.pdf", "pics/cat.png"]);
        let r = run(&root, &idx, "a.md", "[[spec.pdf]] and [[pics/cat.png|a cat]] and [[missing.png]]");
        assert!(r.html.contains(r#"<a href="files/spec.pdf" data-wikilink="true">spec.pdf</a>"#), "{}", r.html);
        assert!(r.html.contains(r#"href="pics/cat.png" data-wikilink="true">a cat</a>"#));
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn standard_images_are_checked_rewritten_and_can_be_sized() {
        let root = project(&["docs/a.png", "shared/b.png"]);
        let idx = index(&["docs/page.md"]);
        let src = "![one](a.png) ![two](/shared/b.png) ![three|120](a.png) ![gone](nope.png) ![web](https://x.test/i.png)";
        let r = run(&root, &idx, "docs/page.md", src);
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
        assert!(r.html.contains(r#"src="a.png" alt="one""#), "{}", r.html);
        assert!(r.html.contains(r#"src="../shared/b.png" alt="two""#));
        assert!(r.html.contains(r#"alt="three" width="120" />"#));
        assert!(r.html.contains(r#"src="https://x.test/i.png""#));
        assert_eq!(r.assets, vec!["docs/a.png", "shared/b.png"]);
    }

    #[test]
    fn references_cannot_escape_the_project() {
        let outer = project(&["secret.png", "proj/ok.png"]);
        let root = outer.join("proj");
        let idx = index(&["a.md"]);
        let r = run(&root, &idx, "a.md", "![x](../secret.png) ![[../secret.png]] ![y](ok.png)");
        assert_eq!(r.assets, vec!["ok.png"]);
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outer.join("secret.png"), root.join("link.png")).unwrap();
            let r = run(&root, &idx, "a.md", "![z](link.png)");
            assert!(r.assets.is_empty(), "a symlink out of the project must not be followed");
        }
    }

    #[test]
    fn links_to_pdfs_and_images_ship_with_the_site() {
        let root = project(&["docs/spec.pdf"]);
        let idx = index(&["docs/a.md"]);
        let r = run(&root, &idx, "docs/a.md", "[spec](spec.pdf#page=2) [other](missing.pdf)");
        assert!(r.html.contains(r#"href="spec.pdf#page=2""#), "{}", r.html);
        assert_eq!(r.assets, vec!["docs/spec.pdf"]);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn size_syntax() {
        assert_eq!(parse_size("300"), Some((300, None)));
        assert_eq!(parse_size("300x200"), Some((300, Some(200))));
        assert_eq!(parse_size("300X200"), Some((300, Some(200))));
        for bad in ["", "x", "0", "300x", "x200", "abc", "3.5", "99999", "-4"] {
            assert_eq!(parse_size(bad), None, "{bad}");
        }
        assert_eq!(split_alt_size("diagram|300"), ("diagram", Some((300, None))));
        assert_eq!(split_alt_size("a|b"), ("a|b", None));
    }

    #[test]
    fn print_target_links_pages_by_in_document_anchor() {
        let root = project(&[]);
        let idx = index(&["a.md", "docs/b.md"]); // ids: p1, p2
        let src = "[[docs/b]] [[b#Some Heading]] [[#Local]] [md](docs/b.md#part) [plain](docs/b.md) [[nope]]";
        let r = run_print(&root, &idx, "a.md", src);
        assert!(r.html.contains(r##"<a href="#p2" data-wikilink="true">b</a>"##), "{}", r.html);
        assert!(r.html.contains(r##"href="#p2--some-heading""##), "{}", r.html);
        assert!(r.html.contains(r##"href="#p1--local""##), "same-page anchors use this page's own id: {}", r.html);
        assert!(r.html.contains(r##"<a href="#p2--part">md</a>"##), "{}", r.html);
        assert!(r.html.contains(r##"<a href="#p2">plain</a>"##), "{}", r.html);
        assert_eq!(r.warnings.len(), 1); // [[nope]]
    }

    #[test]
    fn print_target_namespaces_heading_ids_and_keeps_images_eager() {
        let root = project(&["pic.png"]);
        let idx = index(&["a.md", "b.md"]);
        let r = run_print(&root, &idx, "b.md", "# T\n\n## Overview\n\n![[pic.png|100]]\n");
        assert!(r.has_h1);
        assert!(r.html.contains(r#"id="p2--overview""#), "{}", r.html);
        assert!(!r.html.contains("loading="), "images must not be lazy in a PDF: {}", r.html);
        assert!(r.html.contains(r#"src="pic.png""#), "assets resolve from the document root");
        let site = run(&root, &idx, "b.md", "## Overview\n");
        assert!(site.html.contains(r#"id="overview""#) && !site.has_h1);
    }

    #[test]
    fn print_target_makes_todo_ids_unique_per_page() {
        let root = project(&[]);
        let idx = index(&["a.md", "b.md"]);
        let a = run_print(&root, &idx, "a.md", "TODO one\n");
        let b = run_print(&root, &idx, "b.md", "TODO two\n");
        assert_eq!(a.todos[0].anchor, "p1--todo-1");
        assert_eq!(b.todos[0].anchor, "p2--todo-1");
        assert!(b.html.contains(r#"id="p2--todo-1""#));
    }
}
