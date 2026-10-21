# Themes

`dokt` uses its theme for both the site and PDF, the website allows both light and dark but the PDF is locked to light theme to allow printing. By default the site will match the system default, but you can lock the site to one theme in the config.

## Format

Themes are composed of 8 different colors:

| Key | Used for |
| --- | -------- |
| `bg` | page background |
| `fg` | body text |
| `muted` | secondary text |
| `line` | borders and dividers |
| `soft` | sidebar, code blocks, table headers |
| `accent` | links and the current page |
| `broken` | links to pages that do not exist |
| `todo` | background of highlighted TODO markers |

To change them, set them under `[theme.light]` and `[theme.dark]` in your config file. Any keys not specified will use a default value, so you don't need to define a full theme to modify the colors. To see the full default palette run `dokt init`.

Values must be hex (`#0a5adf`, `#0a5adf80`) or `rgb()`, `hsl()`, `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()` or `color()` notation. Named colors such as `white` are not accepted. If any values are invalid an error will be thrown that will name which key has a bad value.

- `mode = "auto"` (default) follows the operating system's light/dark setting.`"light"` or `"dark"` locks the site to one palette.
- in `auto` mode, a button in the sidebar header cycles the active `mode` (auto -> light -> dark).  `toggle = false`


## Custom CSS

Each file is copied into the site and linked after the built-in styles in the order provided, meaning your rules have priority. The palette keys above are CSS variables (`--bg`, `--fg`, `--accent`, ...), and so are `--sidebar` (sidebar width), `--sans` and `--mono` (font stacks). Only the `.css` file itself is copied: fonts or images it refers to need absolute URLs. Your stylesheets are applied to the PDF as well, so you can provide print styles if desired.


## Config Reference
```toml
# css lives at the top level
css = ["branding/docs.css"]    # paths to additional stylesheets

[theme]
mode = "auto"    # choosing "light" or "dark" forces that theme
toggle = false   # true allows user to pick theme, false with auto forces system preference

# default light theme
[theme.light]
bg =     "#ffffff"
fg =     "#1f2328"
muted =  "#59636e"
line =   "#d8dee4"
soft =   "#f4f6f8"
accent = "#0a5adf"
broken = "#b42318"
todo   = "#fff1b3"

# default dark theme
[theme.dark]
bg =     "#14171a"
fg =     "#dde2e7"
muted =  "#96a1ad"
line =   "#2c333a"
soft =   "#1c2126"
accent = "#79aaff"
broken = "#ff8a80"
todo   = "#5a4a00"
```
