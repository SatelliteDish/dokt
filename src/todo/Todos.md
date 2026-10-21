# Todos

`dokt` tracks all your todos and collects them on the TODOs page. Every todo is also highlighted on the page it appears in. By default the marker that signifies a todo is TODO, but this can be changed in config. Todos have a few special rules to keep in mind:

- TODOs inside code blocks, inline code, link labels, image alt text and HTML comments are ignored.
- The match is a whole word and case-sensitive: `TODO` and `TODO(alice):` match; `TODOs`, `autodoc` and `todo` do not.
- A TODO file (by default any file named `TODO.md`) is different. Every list item in it is an entry: checked task items (`- [x]`) count as done and are struck through. and a checked task that contains the marker (`- [x] TODO ship it`) is done too.
- Several markers in one paragraph make one entry.

## Config Reference

```toml
[todos]
enabled = true                # if disabled TODOs are not tracked
markers = ["TODO", "FIXME"]   # single words: letters, digits, underscore
files   = ["TODO.md"]         # file names, not paths
```
