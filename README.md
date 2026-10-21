# dokt

`dokt`lets you turn Markdown files already scattered through a project into a documentation site. It's
configured using a single file in your project root, and will:

- crawl the project's Markdown files
- resolve Obsidian-style `[[wikilinks]]` and `![[image]]` embeds,
- draw Mermaid diagrams in-place
- highlight TODOs and collect them in one place,
- and give you a static site, a live-reloading preview, and/or a single PDF.

It's one small binary(~8MB) with no runtime dependencies. The generated site is built to work on any host, whether it be locally hosted, on a cloud fileshare, or used as a public documentation website. Find the full documentation [here](https://satellitedish.github.io/dokt/README.html).

## Contents

- [Installation](#installation)
- [Quick start](#quick-start)
- [Commands](#commands)
- [Choosing what is published](#choosing-what-is-published)
- [Keyboard Navigation](#keyboard-navigation)
- [Configuration reference](#configuration-reference)
- [Known limits](#known-limits)

## Installation

You need [Rust](https://www.rust-lang.org/tools/install) 1.89 or newer.

```bash
cargo build --release
```

The compiled binary can be found at `target/release/dokt`. You can test it on the demo project included,
```bash
cd examples/demo
../../target/release/dokt serve
```

## Quick start

```bash
cd my-project
dokt init          # writes dokt.toml with every setting explained
dokt serve         # http://127.0.0.1:3000, rebuilds and reloads as you edit
dokt export --web  # the static site, in dokt-out/ by default
dokt export --pdf  # one PDF, dokt-out/<title>.pdf
```

`dokt.toml` marks the project root. An empty file is valid,  every option has a default it will fall back to. By default every Markdown file in the project becomes a page, and `node_modules/`, `.git/` and `target/` are ignored.

## Commands

Every command finds `dokt.toml` in the current directory or the nearest parent. Use `--config path/to/dokt.toml` (or `-c`) to point at a different one.

| Command | Behavior |
| ------- | ------------ |
| `dokt init [--force]` | Write a starter `dokt.toml` that sets every default value explicitly, explaining each one. |
| `dokt serve [--host H] [--port P] [--out DIR]` | Build and serve (default `127.0.0.1:3000`), rebuild when files change |
| `dokt build [--out DIR] [--strict]` | Build the static site. The same as `export --web`. |
| `dokt export [--web [DIR]] [--pdf [FILE]] [options]` | Export as a static site, a PDF, or both. See. |

`--strict` (on `build` and `export`) turns warnings such as broken links into a failing exit code, which is useful in CI. `serve` notices changes by re-scanning once a second, and also picks up edits to `dokt.toml`, your stylesheets, and your logo.

## Choosing what is published

All paths are relative to `dokt.toml`.

```toml
include = ["README.md", "docs/**", "packages/*/README.md"]   # default: ["**"]
exclude = ["CHANGELOG.md", "docs/drafts/"]                   # gitignore-style
```

- **`include`** is a list of globs. `*` stays inside one folder and `**` crosses folders.
- **`exclude`** uses `.gitignore` syntax, so `drafts/` skips a folder anywhere and `/vendor/` only the one at the root. Exclusion has higher priority than inclusion, so **if a directory matches any exclusion pattern it is never crawled**. This means a large `node_modules` will **not cost anything** if excluded.
- `node_modules/`, `.git/` and `/target/` are skipped by default (`default_excludes`), and `.gitignore` files are honoured (`respect_gitignore`). Dotfiles and dot-folders are skipped. All of this can be changed through config
- Markdown (`.md`, `.markdown`) becomes pages. Images and PDFs that match `include` are copied. You don't need to explicitly include any files actually used in a page, those will be copied regardless of if they're included or not.

`dokt` writes only into `out_dir` (default `dokt-out`). It refuses to write into a non-empty folder it did not create, and never into the project root or one of its parents.

## Keyboard Navigation

| Key | Action |
| --- | ------ |
| `?` | show this shortcut list |
| `/` or `Ctrl+k` or `Cmd+k` | focus the page filter |
| `↑` `↓` | move through filter results |
| `Enter` | open the highlighted result (the first one by default). `Ctrl+Enter`/`Cmd+Enter` opens in a new tab |
| `Esc` | clear the filter |
| `[` `]` | previous / next page, in sidebar order |
| `g` `h` | navigate to the home page |
| `g` `t` | navigate to the TODOs page |
| `t` | cycle the theme |

The filter matches every word you type against page titles and paths. Shortcuts are ignored while you are typing in a field. The first Tab stop on every page is a "Skip to content" link.

## Configuration reference

Every key is optional. Unknown keys are an error, so typos do not pass silently. Run `dokt init`
for a commented starting file.

| Key | Default | Meaning |
| --- | ------- | ------- |
| `title` | folder name | shown in the sidebar, page titles and PDF cover |
| `out_dir` | `"dokt-out"` | where `build`, `serve` and `export` write |
| `include` | `["**"]` | globs of files to consider |
| `exclude` | `[]` | gitignore-style patterns to skip |
| `default_excludes` | `true` | also skip `node_modules/`, `.git/`, `/target/` |
| `respect_gitignore` | `true` | honor `.gitignore` files |
| `allow_html` | `true` | allow raw HTML in Markdown |
| `css` | `[]` | paths to additional stylesheets. Imported with priority over the standard styles |
| **`[todos]`** | | |
| `enabled` | `true` | highlight and collect TODOs |
| `markers` | `["TODO"]` | words that mark a TODO |
| `files` | `["TODO.md"]` | file names whose every list item is a TODO |
| **`[brand]`** | | |
| `logo` | none | image beside the title |
| `logo_dark` | none | logo for the dark theme (needs `logo`) |
| `favicon` | the logo | browser tab icon |
| `show_title` | `true` | `false` shows only the logo |
| `footer` | none | plain text at the bottom of every page |
| **`[export]`** | | |
| `paper` | `"a4"` | `"a4"` or `"letter"` |
| `browser` | found automatically | path to Chrome, Chromium or Edge |
| **`[theme]`** | | |
| `mode` | `"auto"` | `"auto"`, `"light"` or `"dark"` |
| `toggle` | `true` | show the theme button (only in `auto` mode) |
| `[theme.light]`, `[theme.dark]` | built in | `bg` `fg` `muted` `line` `soft` `accent` `broken` `todo` |


## Known limits

- The PDF's table of contents page has links but not page numbers . Browsers cannot compute them for a printed page.
- Note embedding (`![[some note]]`) is not supported.
- The theme colors do not recolor Mermaid diagrams (they use Mermaid's own light or dark theme), and the `[!TIP]` and `[!IMPORTANT]` alert colors are fixed. This can be overridden manually using additional CSS
- The favicon is one image for both themes.
- The sidebar, theme button and shortcuts need JavaScript. The pages themselves read fine without it.
- `serve` detects changes by re-scanning once a second rather than using file-system events. In testing this seems to have minimal impact on performance and should be more resilient across environments
- Tested on Linux only. The browser locations for macOS and Windows are the common install paths but have not been tried, and Windows path handling is unverified.
- Sidebar necessarily mirrors directory structure. If you have deeply nested files, say in a Java repository, it might be frustrating to click through each one.
- Certain static files cannot be uploaded included in the site, such as a `robots.txt`

