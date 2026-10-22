# Config

Everything in `dokt` is a controlled with a single TOML config file. If you aren't familiar with TOML syntax you can read more [here](https://toml.io/en/). Every key in the config file is optional, there are defaults in place for every value. Unknown keys are an error, so **typos do not pass silently**. Run `dokt init` to generate a config file where each default is set *explicitly* and each setting is explained.

## Top-level
These have to come before any of the config below.

| Key | Default | Meaning |
| --- | ------- | ------- |
| `title` | folder name | shown in the sidebar, page titles and PDF cover |
| `out_dir` | `"dokt-out"` | where `build`, `serve` and `export` write |
| `include` | `["**"]` | globs of files to consider |
| `exclude` | `[]` | gitignore-style patterns to skip |
| `default_excludes` | `true` | also skip `node_modules/`, `.git/`, `/target/` |
| `respect_gitignore` | `true` | honor `.gitignore` files |
| `allow_html` | `true` | allow raw HTML to pass through Markdown |
| `css` | `[]` | paths to additional stylesheets. Imported with priority over the standard styles |


### Choosing what is published

- **`include`** is a list of globs. `*` stays inside one folder and `**` crosses folders.
- **`exclude`** uses `.gitignore` syntax, so `drafts/` skips a folder named drafts anywhere and `/vendor/` only one if it's at the root.
- Exclusion has higher priority than inclusion, so **if a directory matches any exclusion pattern it is never crawled**. This means a large `node_modules` will **not cost anything** if excluded.
- `node_modules/`, `.git/` and `/target/` are skipped by default (`default_excludes`), and `.gitignore` files are honoured (`respect_gitignore`). Dotfiles and dot-folders are skipped. All of this can be changed through config
- Markdown (`.md`, `.markdown`) becomes pages. Images and PDFs that match `include` are copied. You don't need to explicitly include any files actually used in a page, those will be copied regardless of if they're included or not.

All paths are relative to `dokt.toml`. `dokt` writes only into `out_dir`. It refuses to write into a non-empty folder it did not create, and never into the project root or one of its parents.

### Example

```toml
title = "My app"
out_dir = "docs_out"
respect_gitignore = false
css = ["assets/globals.css"]
```

## TODOs
These options are part of the `[todos]` table. They offer control over how the TODOs are parsed and understood. See [[Todos]] for more

| Key | Default | Meaning |
| --- | ------- | ------- |
| `enabled` | `true` | highlight and collect TODOs |
| `markers` | `["TODO"]` | words that mark a TODO |
| `files` | `["TODO.md"]` | file names whose every list item is a TODO |

### Example

```toml
[todos]
enabled = true
files = ["TaskList.md"]
```

## Brand
These options are a part of the `[brand]` table. The allow you to control the branding elements for your generated documentation, see [[Branding]] for more information.

| Key | Default | Meaning |
| --- | ------- | ------- |
| `logo` | none | image beside the title |
| `logo_dark` | none | logo for the dark theme (needs `logo`) |
| `favicon` | the logo | browser tab icon |
| `show_title` | `true` | `false` shows only the logo |
| `footer` | none | plain text at the bottom of every page |

### Example

```toml
[brand]
logo = "assets/logo-black.png"
logo_dark = "assets/logo-white.png"
footer = "Copyright 2026 Acme Inc."
```


## Export
These options are a part of the `[export]` table. This lets you control how the documentation is exported as a PDF, see [[Export]] for more information.

| Key | Default | Meaning |
| --- | ------- | ------- |
| `paper` | `"a4"` | `"a4"` or `"letter"` |
| `browser` | found automatically | path to Chrome, Chromium or Edge |

### Example

```toml
[export]
paper = "letter"
browser = "/usr/bin/google-chrome-bin"
```

## Theme
These options are a part of the `[theme]` table. These let you change the colors used in your generated website and/or PDF, see [[Themes]] for more information. PDFs always use the light theme. See [[Export]] for more information on exported PDFs

| Key | Default | Meaning |
| --- | ------- | ------- |
| `mode` | `"auto"` | `"auto"`, `"light"` or `"dark"` |
| `toggle` | `true` | show the theme button (only in `auto` mode) |
| `[theme.light]`, `[theme.dark]` | built in | `bg` `fg` `muted` `line` `soft` `accent` `broken` `todo` |

### Example

```toml
[theme]
mode = "auto"
toggle = true

[theme.dark]
bg = "#01000C"
fg = "#E3ECF7"
accent = "#F7AC42"
```
