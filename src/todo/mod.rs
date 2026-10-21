//! TODO detection on a parsed page.
//!
//! Works on the document tree, not the raw text, so TODOs inside code blocks,
//! inline code, link labels and image alt text are ignored. Markers are
//! highlighted in place with `<mark>` and each block that contains one yields
//! a [`TodoEntry`] pointing back at it.
//!
//! In a TODO file (e.g. `TODO.md`) every list item is an entry instead, and
//! checked task items are reported as done.

use crate::config::Todos;
use crate::markdown::file_name;
use comrak::nodes::{AstNode, NodeValue};
use comrak::Arena;
use std::collections::HashSet;

/// One TODO found on a page.
pub struct TodoEntry {
    /// The text of the block that holds the TODO, whitespace collapsed.
    pub text: String,
    /// 1-based line in the source file.
    pub line: usize,
    /// Element id to link to, e.g. `dokt-todo-3`.
    pub anchor: String,
    pub done: bool,
}

/// True if the file's name is one of the configured TODO files (case-insensitive).
pub fn is_todo_file(rel: &str, todos: &Todos) -> bool {
    let name = file_name(rel).to_lowercase();
    todos.files.iter().any(|f| f.to_lowercase() == name)
}

/// Find TODOs in a parsed page, highlight them in place, and return one entry
/// per block that contains one.
///
/// `id_prefix` is put in front of the generated element ids (`<prefix>todo-3`).
/// It is `dokt-` on the site and `<page id>--` in a single-document export,
/// where ids must be unique across every page.
pub fn process<'a>(
    arena: &'a Arena<'a>,
    root: &'a AstNode<'a>,
    todos: &Todos,
    file_mode: bool,
    id_prefix: &str,
) -> Vec<TodoEntry> {
    let mut entries: Vec<TodoEntry> = Vec::new();
    let mut counter = 0usize;
    // Blocks that already have an entry (or, in a TODO file, are list items).
    let mut claimed: HashSet<usize> = HashSet::new();
    let all: Vec<&'a AstNode<'a>> = root.descendants().collect();

    // ---- 1. TODO files: every list item is a TODO ----
    if file_mode {
        for node in &all {
            let done = match &node.data().value {
                NodeValue::TaskItem(t) => t.symbol.is_some(),
                NodeValue::Item(_) => false,
                _ => continue,
            };
            let Some(para) = node
                .children()
                .find(|c| matches!(c.data().value, NodeValue::Paragraph))
            else {
                continue;
            };
            let text = block_text(para);
            if text.is_empty() {
                continue;
            }
            counter += 1;
            let anchor = anchor_id(id_prefix, counter);
            if let Some(first) = para.first_child() {
                first.insert_before(arena.alloc(
                    NodeValue::Raw(format!(r#"<span id="{anchor}"></span>"#)).into(),
                ));
            }
            claimed.insert(addr(para));
            entries.push(TodoEntry {
                text: truncate(&text),
                line: para.data().sourcepos.start.line,
                anchor,
                done,
            });
        }
    }

    // ---- 2. marker words in running text ----
    for node in &all {
        let text = match &node.data().value {
            NodeValue::Text(t) => t.to_string(),
            _ => continue,
        };
        let spans = find_markers(&text, &todos.markers);
        if spans.is_empty() {
            continue;
        }

        let mut block: Option<&'a AstNode<'a>> = None;
        let mut skip = false;
        let mut done = false;
        for ancestor in node.ancestors().skip(1) {
            let data = ancestor.data();
            match &data.value {
                NodeValue::Link(_) | NodeValue::WikiLink(_) | NodeValue::Image(_) => {
                    skip = true;
                    break;
                }
                NodeValue::TaskItem(t) if t.symbol.is_some() => done = true,
                v if block.is_none() && v.block() => block = Some(ancestor),
                _ => {}
            }
        }
        if skip {
            continue;
        }

        let block_key = block.map(|b| addr(b)).unwrap_or_else(|| addr(node));
        let mut id: Option<String> = None;
        if !claimed.contains(&block_key) {
            claimed.insert(block_key);
            counter += 1;
            let anchor = anchor_id(id_prefix, counter);
            let entry_text = block.map(|b| block_text(b)).unwrap_or_else(|| text.trim().to_string());
            let line = block.map(|b| b.data().sourcepos.start.line).unwrap_or(0);
            entries.push(TodoEntry { text: truncate(&entry_text), line, anchor: anchor.clone(), done });
            id = Some(anchor);
        }

        // Replace the text node with: text, <mark>, marker, </mark>, text, ...
        let mut cursor = 0;
        for (i, (start, end)) in spans.iter().enumerate() {
            if *start > cursor {
                insert_text(arena, node, &text[cursor..*start]);
            }
            let open = match (&id, i) {
                (Some(a), 0) => format!(r#"<mark class="dokt-todo" id="{a}">"#),
                _ => r#"<mark class="dokt-todo">"#.to_string(),
            };
            node.insert_before(arena.alloc(NodeValue::Raw(open).into()));
            insert_text(arena, node, &text[*start..*end]);
            node.insert_before(arena.alloc(NodeValue::Raw("</mark>".to_string()).into()));
            cursor = *end;
        }
        if cursor < text.len() {
            insert_text(arena, node, &text[cursor..]);
        }
        node.detach();
    }

    entries.sort_by_key(|e| e.line);
    entries
}

/// Insert a text node before `before`.
fn insert_text<'a>(arena: &'a Arena<'a>, before: &'a AstNode<'a>, text: &str) {
    before.insert_before(arena.alloc(NodeValue::Text(text.to_string().into()).into()));
}

/// The element id for TODO number `n`, e.g. `dokt-todo-3`.
fn anchor_id(prefix: &str, n: usize) -> String {
    format!("{prefix}todo-{n}")
}

/// A node's address, used as an identity key (nodes have no id of their own).
fn addr<'a>(node: &'a AstNode<'a>) -> usize {
    node as *const AstNode<'a> as usize
}

/// Visible text of a block, with whitespace collapsed.
fn block_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut s = String::new();
    for d in node.descendants() {
        match &d.data().value {
            NodeValue::Text(t) => s.push_str(t),
            NodeValue::Code(c) => s.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => s.push(' '),
            _ => {}
        }
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Shorten long entries for the TODO list.
fn truncate(s: &str) -> String {
    const MAX: usize = 200;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let cut: String = s.chars().take(MAX).collect();
        format!("{}…", cut.trim_end())
    }
}

/// Byte ranges of whole-word, case-sensitive marker occurrences.
pub fn find_markers(text: &str, markers: &[String]) -> Vec<(usize, usize)> {
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for marker in markers {
        for (start, _) in text.match_indices(marker.as_str()) {
            let end = start + marker.len();
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            if before.map_or(false, is_word) || after.map_or(false, is_word) {
                continue;
            }
            spans.push((start, end));
        }
    }
    spans.sort();
    spans.dedup();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for s in spans {
        if out.last().map_or(true, |last| s.0 >= last.1) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::{render, Ctx, Index, PageInfo};
    use std::path::Path;

    fn index_for(rel: &str) -> Index {
        Index::new(
            vec![PageInfo {
                rel: rel.into(),
                html: format!("{}.html", crate::markdown::strip_md_ext(rel)),
                stem: crate::markdown::stem_of(rel).into(),
                id: "p1".into(),
            }],
            vec![],
        )
    }

    fn render_with(rel: &str, src: &str, todos: &Todos) -> crate::markdown::Rendered {
        let idx = index_for(rel);
        let ctx = Ctx {
            index: &idx,
            root: Path::new("/nonexistent"),
            allow_html: true,
            todos,
            target: crate::markdown::Target::Site,
        };
        render(src, rel, "x.html", &ctx)
    }

    fn run(rel: &str, src: &str) -> (Vec<TodoEntry>, String) {
        let r = render_with(rel, src, &Todos::default());
        (r.todos, r.html)
    }

    #[test]
    fn finds_whole_word_case_sensitive_markers() {
        let m = vec!["TODO".to_string()];
        assert_eq!(find_markers("TODO: x", &m), vec![(0, 4)]);
        assert_eq!(find_markers("a TODO(bob) and TODO", &m).len(), 2);
        assert!(find_markers("TODOs autodoc TODO_X xTODO todo To-do", &m).is_empty());
        assert_eq!(find_markers("(TODO)", &m), vec![(1, 5)]);
        assert_eq!(find_markers("ÉTODO TODOé", &m), vec![]); // letters on either side
    }

    #[test]
    fn highlights_in_place_and_records_line_and_anchor() {
        let (todos, html) = run("a.md", "# Title\n\nSome text.\n\nTODO: write the auth section\n");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].line, 5);
        assert_eq!(todos[0].text, "TODO: write the auth section");
        assert_eq!(todos[0].anchor, "dokt-todo-1");
        assert!(!todos[0].done);
        assert!(html.contains(r#"<mark class="dokt-todo" id="dokt-todo-1">TODO</mark>: write the auth section"#), "{html}");
    }

    #[test]
    fn ignores_code_links_and_images() {
        let src = "Use `TODO` here.\n\n```\nTODO in a fence\n```\n\n[TODO list](x.md) and ![TODO](i.png)\n\n<!-- TODO in a comment -->\n";
        let (todos, html) = run("a.md", src);
        assert!(todos.is_empty(), "{:?}", todos.iter().map(|t| &t.text).collect::<Vec<_>>());
        assert!(!html.contains("dokt-todo"));
    }

    #[test]
    fn several_markers_in_one_paragraph_make_one_entry_with_one_id() {
        let (todos, html) = run("a.md", "TODO first and TODO second\n");
        assert_eq!(todos.len(), 1);
        assert_eq!(html.matches("<mark").count(), 2);
        assert_eq!(html.matches("id=\"dokt-todo-1\"").count(), 1);
    }

    #[test]
    fn finds_todos_in_headings_lists_and_tables() {
        let src = "## TODO revisit\n\n- item one\n- TODO list thing\n\n| a | b |\n|---|---|\n| TODO cell | ok |\n";
        let (todos, _) = run("a.md", src);
        let texts: Vec<_> = todos.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["TODO revisit", "TODO list thing", "TODO cell"]);
    }

    #[test]
    fn heading_slug_still_includes_the_marker_word() {
        let (_, html) = run("a.md", "## TODO revisit\n");
        assert!(html.contains(r#"id="todo-revisit""#), "{html}");
    }

    #[test]
    fn checked_task_with_marker_is_done() {
        let (todos, _) = run("a.md", "- [x] TODO already shipped\n- [ ] TODO still open\n");
        assert_eq!((todos[0].done, todos[1].done), (true, false));
    }

    #[test]
    fn todo_file_turns_every_list_item_into_an_entry() {
        let src = "# Plan\n\n- [ ] write docs\n- [x] set up CI\n- plain bullet\n  - nested [ ] not a task\n\nTODO: general note\n";
        let (todos, html) = run("TODO.md", src);
        let got: Vec<_> = todos.iter().map(|t| (t.text.as_str(), t.done)).collect();
        assert_eq!(
            got,
            vec![
                ("write docs", false),
                ("set up CI", true),
                ("plain bullet", false),
                ("nested [ ] not a task", false),
                ("TODO: general note", false),
            ]
        );
        assert!(html.contains(r#"<span id="dokt-todo-1"></span>"#));
        // The marker in the ordinary paragraph is highlighted and collected once.
        assert_eq!(html.matches("<mark").count(), 1);
    }

    #[test]
    fn marker_inside_a_todo_file_list_item_is_highlighted_but_not_duplicated() {
        let (todos, html) = run("docs/todo.md", "- TODO fix this\n- other\n");
        assert_eq!(todos.len(), 2);
        assert!(html.contains("<mark"));
    }

    #[test]
    fn todo_file_matching_is_by_file_name_case_insensitively() {
        let t = Todos::default();
        assert!(is_todo_file("TODO.md", &t));
        assert!(is_todo_file("docs/todo.md", &t));
        assert!(!is_todo_file("docs/todo-list.md", &t));
        assert!(!is_todo_file("TODO.md/readme.md", &t));
    }

    #[test]
    fn custom_markers_and_disabling() {
        let cfg = Todos { enabled: true, markers: vec!["FIXME".into()], files: vec![] };
        let r = render_with("a.md", "TODO nope\n\nFIXME yes\n", &cfg);
        assert_eq!(r.todos.len(), 1);
        let off = Todos { enabled: false, ..Todos::default() };
        let r = render_with("a.md", "TODO nope\n", &off);
        assert!(r.todos.is_empty() && !r.html.contains("<mark"));
    }
}
