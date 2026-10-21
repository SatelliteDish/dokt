# Branding

`dokt` gives you a number of ways to integrate brand identity into the generated docs. This section covers all the content-based options, if you want to customize the styles check out [[Themes]]. Branding is information like the site footer, logo, or favicon. All files are copied into `assets/brand/`. Logo height defaults to `1.7rem`, this can be changed by passing `--logo-height` or using [[Themes#Custom CSS|custom CSS]].

Branding is also used in exported PDF files. The logo and footer appear on the title page, though the favicon is not used.

## Config Reference

```toml
[brand]
logo       = "branding/logo.svg"        # shown next to the site title
logo_dark  = "branding/logo-dark.svg"   # optional: used instead in the dark theme
favicon    = "branding/favicon.png"     # optional: defaults to the logo
show_title = true                       # false shows only the logo (always shown screen readers)
footer     = "(c) 2026 Your Name"       # plain text at the bottom of every page
```
