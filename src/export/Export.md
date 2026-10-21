# Export

`dokt` has the ability to export documentation either as a [[#Static site|static site]] or a [[#PDF|standalone PDF]].

```bash
dokt export --web              # static site into out_dir
dokt export --web public       # ...or into ./public
dokt export --pdf              # PDF into out_dir, named after the title
dokt export --pdf manual.pdf   # ...or to a file of your choice
dokt export --web --pdf        # both
```

At least one of `--web` or `--pdf` is required.

## Static site

`export --web` is the same as `dokt build`. The output is plain HTML, CSS and JavaScript with
relative links. The goal is to provide output that can serve both as a local dev reference and a public documentation website under a domain name.

## PDF

The PDF is one document built from the same pages as the site:

1. a **cover** with the logo, title, page count and generation date
2. a **table of contents** page, laid out like the sidebar
3. **every page** in sidebar order, each on a new sheet
4. an **appendix** that lists every TODO with links

Links work in the PDF as well, links between pages will just take you to the relevant page of the document. It always uses your light palette so it can be printed.

In order to get the PDF to include everything from the website (diagrams especially) **we need to use a browser**. `dokt` does not come with a browser, it uses one **you already have installed**.

### CLI Options

| Option | Meaning |
| ------ | ------- |
| `--paper a4\|letter` | paper size. Default specified in as `[export] paper` in config, else `a4` |
| `--browser PATH` | the browser to print with, can also be set using `DOKT_BROWSER` environment variable |
| `--no-sandbox` | run the browser without its sandbox. **Required when running as root**, e.g. in Docker or some CI |
| `--strict` | fail on warnings, including diagrams that could not be drawn |


If no brower is specified it uses an installed version of Chrome, Chromium or Edge found on `PATH` or in the usual locations. A diagram with a syntax error is printed as its source text, marked in red, and warning is printed to the console.

## Config Reference

```toml
[export]
paper = "letter"                # Paper size for PDF, "letter" or "a4
browser = "/usr/bin/chromium"   # Path to browser to use for PDF rendering
```
