# dokt

`dokt` lets you turn Markdown files already scattered through a project into a documentation site. It's
configured using a single file in your project root, and will:

- crawl the project's Markdown files
- resolve Obsidian-style `[[wikilinks]]` and `![[image]]` embeds,
- draw Mermaid diagrams in-place
- highlight TODOs and collect them in one place,
- and give you a static site, a live-reloading preview, and/or a single PDF.

It's one small binary(~8MB) with no runtime dependencies. The generated site is built to work on any host, whether it be locally hosted, on a cloud fileshare, or used as a public documentation website. To see a demo or find more information you can view the full docs [here](https://satellitedish.github.io/dokt/README.html), which are generated with `dokt`.

## Contents

- [Installation](#installation)
    - [Npm](#npm)
    - [Cargox](#cargox)
    - [Source](#source)
- [Quick start](#quick-start)
- [Commands](#commands)
- [Keyboard Navigation](#keyboard-navigation)
- [Configuration reference](#configuration-reference)
- [Known limits](#known-limits)

### See Also

- [Configuration](src/config/Config.md)
- [Branding](/src/config/Branding.md)
- [Themes](src/theme/Themes.md)

## Installation

### npm
There are two ways to install using `npm`:

```bash
npx @sortofrad/dokt init   # you do not need to add `dokt` as a dependency to use it

# or

npm install --save-dev @sortofrad/dokt
npx dokt init    # adding as a dependency allows you to use the shorter name
```

It's recommended to add `dokt` as a dependency for ease of use, but you do not have to.

### cargox
It's recommended to use `dokt` through [`cargox`](https://github.com/pkgxdev/cargox) for Rust projects:

```bash
carogx dokt init
```

And that's it. You can install `dokt` as a dependency with `cargo install --dev dokt` but there isn't any benefit to doing so.

### Source
To install from source you need [Rust](https://www.rust-lang.org/tools/install) 1.89 or newer.

```bash
cargo build --release
```

The compiled binary can be found at `target/release/dokt`. You can test it out on the source itself:

```bash
./target/release/dokt serve
```

## Quick start

```bash
cd my-project
dokt init          # writes dokt.toml with every setting explained
dokt serve         # http://127.0.0.1:3000, rebuilds and reloads as you edit
dokt export --web  # the static site, in dokt-out/ by default
dokt export --pdf  # one PDF, dokt-out/<title>.pdf
```

`dokt.toml` marks the project root. An empty file is valid, every option has a default it will fall back to. By default every Markdown file in the project becomes a page, and by default `node_modules/`, `.git/`, and `target/` are ignored. You can find a full list configuration options and default values [here](/src/config/Config.md).

## Commands

Every command finds `dokt.toml` in the current directory or the nearest parent. Use `--config path/to/dokt.toml` (or `-c`) to point at a different one.

| Command | Behavior |
| ------- | ------------ |
| `dokt init [--force]` | Write a starter `dokt.toml` that sets every default value explicitly, explaining each one. |
| `dokt serve [--host H] [--port P] [--out DIR]` | Build and serve (default `127.0.0.1:3000`), rebuild when files change |
| `dokt build [--out DIR] [--strict]` | Build the static site. The same as `export --web`. |
| `dokt export [--web [DIR]] [--pdf [FILE]] [options]` | Export as a static site, a PDF, or both. See. |

`--strict` (on `build` and `export`) turns warnings such as broken links into a failing exit code, which is useful in CI. `serve` notices changes by re-scanning once a second, and also picks up edits to `dokt.toml`, your stylesheets, and your logo.


## Keyboard Navigation

The generated website comes with a number of keybinds for easy keyboard navigation.

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

Every key is optional. Unknown keys are an error, so typos do not pass silently. Run `dokt init` for a commented starting file.

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

