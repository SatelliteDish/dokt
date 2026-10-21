/* Runs on every page of a dokt site: plain JavaScript, no dependencies.
 *
 * It reads what the build left on the page:
 *   <body data-root>   path from this page up to the site root ("../")
 *   <body data-page>   this page's output path ("docs/guide/setup.html")
 *   window.DOKT_NAV    [[outputPath, title, sourcePath], ...] for the sidebar
 *   window.DOKT_TODOS  { href, open, done }, or null when TODOs are off
 *   window.DOKT_TOGGLE true when the theme button is enabled
 *
 * and then builds the sidebar tree and filter box, the theme button, the link to
 * the TODOs page and the keyboard shortcuts. Mermaid is only loaded on pages
 * that contain a diagram.
 */
(function () {
  "use strict";
  var doc = document;
  var html = doc.documentElement;
  var body = doc.body;
  var root = body.getAttribute("data-root") || "";
  var current = body.getAttribute("data-page") || "";
  var nav = window.DOKT_NAV || []; // [[outputHref, title, sourcePath], ...]
  var todos = window.DOKT_TODOS || null; // { href, open, done } or null
  var canToggle = window.DOKT_TOGGLE === true;
  var STORE = "dokt-theme";

  var navHead = doc.querySelector("#nav .nav-head");
  var filterEl = doc.getElementById("filter");
  var treeEl = doc.getElementById("tree");

  function url(href) {
    return root + href.split("/").map(encodeURIComponent).join("/");
  }
  function byName(a, b) {
    return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
  }
  function isTyping(t) {
    return !!t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.tagName === "SELECT" || t.isContentEditable);
  }

  // ======================= theme (auto / light / dark) =======================
  var pref = null; // null = follow the operating system
  try {
    var saved = localStorage.getItem(STORE);
    if (canToggle && (saved === "light" || saved === "dark")) { pref = saved; }
  } catch (e) { /* storage unavailable: just follow the OS */ }

  function osDark() {
    return !!(window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches);
  }
  function resolvedTheme() {
    return html.getAttribute("data-theme") || (osDark() ? "dark" : "light");
  }

  var ICONS = {
    auto: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"></circle><path d="M12 3v18a9 9 0 0 0 0-18z" fill="currentColor"></path></svg>',
    light: '<svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="4"></circle><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"></path></svg>',
    dark: '<svg viewBox="0 0 24 24" aria-hidden="true"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"></path></svg>'
  };
  var NEXT = { auto: "light", light: "dark", dark: "auto" };
  var LABEL = { auto: "Auto (follows your system)", light: "Light", dark: "Dark" };
  var toggleBtn = null;

  function prefName() { return pref === null ? "auto" : pref; }

  function updateToggle() {
    if (!toggleBtn) { return; }
    var cur = prefName();
    toggleBtn.innerHTML = ICONS[cur];
    var text = "Theme: " + LABEL[cur] + ". Switch to " + LABEL[NEXT[cur]].split(" ")[0] + ".";
    toggleBtn.setAttribute("aria-label", text);
    toggleBtn.title = text + " (t)";
  }

  function setPref(next) {
    pref = next;
    try {
      if (next) { localStorage.setItem(STORE, next); } else { localStorage.removeItem(STORE); }
    } catch (e) { /* ignore */ }
    if (next) { html.setAttribute("data-theme", next); } else { html.removeAttribute("data-theme"); }
    updateToggle();
    renderMermaid();
  }
  function cycleTheme() {
    var n = NEXT[prefName()];
    setPref(n === "auto" ? null : n);
  }

  if (canToggle && navHead) {
    toggleBtn = doc.createElement("button");
    toggleBtn.id = "theme-toggle";
    toggleBtn.type = "button";
    toggleBtn.setAttribute("aria-keyshortcuts", "t");
    toggleBtn.addEventListener("click", cycleTheme);
    navHead.appendChild(toggleBtn);
    updateToggle();
  }

  // ======================= link to the TODOs page =======================
  if (todos && navHead) {
    var link = doc.createElement("a");
    link.className = "todo-link";
    link.href = root + todos.href;
    if (todos.href === current) { link.setAttribute("aria-current", "page"); }
    var label = doc.createElement("span");
    label.textContent = "TODOs";
    var badge = doc.createElement("span");
    badge.className = "badge";
    badge.textContent = String(todos.open);
    badge.title = todos.open + " open, " + todos.done + " done";
    link.appendChild(label);
    link.appendChild(badge);
    navHead.insertAdjacentElement("afterend", link);
  }

  // ======================= sidebar tree =======================
  var leaves = []; // { li, a, href, text } in the order they appear on screen
  var folders = []; // { li, details, wasOpen }

  function group(items) {
    var top = { dirs: {}, files: [] };
    items.forEach(function (it) {
      var parts = it[2].split("/");
      var node = top;
      for (var i = 0; i < parts.length - 1; i++) {
        node = node.dirs[parts[i]] = node.dirs[parts[i]] || { dirs: {}, files: [] };
      }
      node.files.push(it);
    });
    return top;
  }

  function build(node) {
    var ul = doc.createElement("ul");
    var hasCurrent = false;
    node.files
      .sort(function (a, b) { return byName(a[2], b[2]); })
      .forEach(function (it) {
        var li = doc.createElement("li");
        var a = doc.createElement("a");
        a.href = url(it[0]);
        a.id = "leaf-" + leaves.length;
        a.textContent = it[1];
        if (it[0] === current) { a.setAttribute("aria-current", "page"); hasCurrent = true; }
        li.appendChild(a);
        ul.appendChild(li);
        leaves.push({ li: li, a: a, href: it[0], text: (it[1] + " " + it[2]).toLowerCase() });
      });
    Object.keys(node.dirs).sort(byName).forEach(function (name) {
      var child = build(node.dirs[name]);
      var li = doc.createElement("li");
      var details = doc.createElement("details");
      var summary = doc.createElement("summary");
      summary.textContent = name;
      details.appendChild(summary);
      details.appendChild(child.ul);
      if (child.hasCurrent) { details.open = true; hasCurrent = true; }
      li.appendChild(details);
      ul.appendChild(li);
      folders.push({ li: li, details: details, wasOpen: details.open });
    });
    return { ul: ul, hasCurrent: hasCurrent };
  }

  if (treeEl) {
    treeEl.appendChild(build(group(nav)).ul);
    var active = treeEl.querySelector('[aria-current="page"]');
    if (active && active.scrollIntoView) { active.scrollIntoView({ block: "center" }); }
  }

  // ======================= filter box =======================
  var visible = []; // leaves currently shown while filtering
  var activeIdx = -1; // which of `visible` Enter will open
  var status = null;

  if (filterEl) {
    status = doc.createElement("div");
    status.className = "sr-only";
    status.setAttribute("aria-live", "polite");
    filterEl.insertAdjacentElement("afterend", status);
  }

  function setActive(i) {
    leaves.forEach(function (l) { l.a.classList.remove("kb-active"); });
    activeIdx = i >= 0 && i < visible.length ? i : -1;
    if (activeIdx >= 0) {
      var a = visible[activeIdx].a;
      a.classList.add("kb-active");
      filterEl.setAttribute("aria-activedescendant", a.id);
      if (a.scrollIntoView) { a.scrollIntoView({ block: "nearest" }); }
    } else {
      filterEl.removeAttribute("aria-activedescendant");
    }
  }

  function applyFilter() {
    var terms = filterEl.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    var filtering = terms.length > 0;
    leaves.forEach(function (l) {
      var show = !filtering || terms.every(function (t) { return l.text.indexOf(t) !== -1; });
      l.li.classList.toggle("hidden", !show);
    });
    for (var i = 0; i < folders.length; i++) {
      var f = folders[i];
      var anyVisible = f.li.querySelector("li:not(.hidden) > a") !== null;
      f.li.classList.toggle("hidden", filtering && !anyVisible);
      f.details.open = filtering ? anyVisible : f.wasOpen;
    }
    visible = filtering ? leaves.filter(function (l) { return !l.li.classList.contains("hidden"); }) : [];
    setActive(filtering ? 0 : -1); // Enter opens the first match unless you move
    if (status) {
      status.textContent = filtering ? visible.length + (visible.length === 1 ? " page matches" : " pages match") : "";
    }
  }

  function focusFilter() {
    if (!filterEl) { return; }
    filterEl.focus();
    filterEl.select();
  }

  if (filterEl) {
    filterEl.addEventListener("input", applyFilter);
    filterEl.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown" && visible.length) {
        e.preventDefault();
        setActive(Math.min(activeIdx + 1, visible.length - 1));
      } else if (e.key === "ArrowUp" && visible.length) {
        e.preventDefault();
        setActive(Math.max(activeIdx - 1, 0));
      } else if (e.key === "Enter" && activeIdx >= 0) {
        e.preventDefault();
        var target = visible[activeIdx].a.href;
        if (e.ctrlKey || e.metaKey) { window.open(target, "_blank", "noopener"); } else { window.location.href = target; }
      } else if (e.key === "Escape") {
        if (filterEl.value) { filterEl.value = ""; applyFilter(); } else { filterEl.blur(); }
        e.preventDefault();
      }
    });
  }

  // ======================= keyboard shortcuts =======================
  var helpDialog = null;
  function openHelp() {
    if (!helpDialog) {
      helpDialog = doc.createElement("dialog");
      helpDialog.id = "help";
      helpDialog.setAttribute("aria-label", "Keyboard shortcuts");
      var rows = [
        ["<kbd>/</kbd> or <kbd>Ctrl</kbd>/<kbd>\u2318</kbd> <kbd>K</kbd>", "Focus the page filter"],
        ["<kbd>\u2191</kbd> <kbd>\u2193</kbd>", "Move through filter results"],
        ["<kbd>Enter</kbd>", "Open the highlighted result (first by default); with Ctrl/\u2318, in a new tab"],
        ["<kbd>Esc</kbd>", "Clear the filter, then leave it"],
        ["<kbd>[</kbd> <kbd>]</kbd>", "Previous / next page in the sidebar"],
        ["<kbd>g</kbd> <kbd>h</kbd>", "Go to the home page"]
      ];
      if (todos) { rows.push(["<kbd>g</kbd> <kbd>t</kbd>", "Go to the TODOs page"]); }
      if (canToggle) { rows.push(["<kbd>t</kbd>", "Switch theme: auto, light, dark"]); }
      rows.push(["<kbd>?</kbd>", "Show this help"]);
      var dl = rows.map(function (r) { return "<dt>" + r[0] + "</dt><dd>" + r[1] + "</dd>"; }).join("");
      helpDialog.innerHTML = "<h2>Keyboard shortcuts</h2><dl>" + dl + '</dl><button type="button">Close</button>';
      helpDialog.querySelector("button").addEventListener("click", function () { helpDialog.close(); });
      helpDialog.addEventListener("click", function (e) { if (e.target === helpDialog) { helpDialog.close(); } });
      body.appendChild(helpDialog);
    }
    if (typeof helpDialog.showModal === "function" && !helpDialog.open) { helpDialog.showModal(); }
  }

  function go(href) { window.location.href = href; }
  function step(delta) {
    for (var i = 0; i < leaves.length; i++) {
      if (leaves[i].href === current) {
        var next = leaves[i + delta];
        if (next) { go(next.a.href); }
        return;
      }
    }
  }

  var pendingG = false;
  var gTimer = null;
  doc.addEventListener("keydown", function (e) {
    if (e.defaultPrevented) { return; }
    var mod = e.ctrlKey || e.metaKey;
    if (mod && !e.altKey && !e.shiftKey && (e.key === "k" || e.key === "K")) {
      e.preventDefault();
      focusFilter();
      return;
    }
    if (mod || e.altKey || isTyping(e.target) || (helpDialog && helpDialog.open)) { return; }

    if (pendingG) {
      pendingG = false;
      clearTimeout(gTimer);
      if (e.key === "h") { e.preventDefault(); go(root + "index.html"); }
      else if (e.key === "t" && todos) { e.preventDefault(); go(root + todos.href); }
      return;
    }
    switch (e.key) {
      case "/": e.preventDefault(); focusFilter(); break;
      case "?": e.preventDefault(); openHelp(); break;
      case "t": if (canToggle) { e.preventDefault(); cycleTheme(); } break;
      case "[": step(-1); break;
      case "]": step(1); break;
      case "g":
        pendingG = true;
        gTimer = setTimeout(function () { pendingG = false; }, 1200);
        break;
    }
  });

  // ======================= Mermaid (loaded only on pages with a diagram) =======================
  var diagrams = Array.prototype.slice.call(doc.querySelectorAll("pre.mermaid"));
  diagrams.forEach(function (el) { el.setAttribute("data-src", el.textContent); });

  function renderMermaid() {
    if (!diagrams.length || !window.mermaid) { return; }
    window.mermaid.initialize({
      startOnLoad: false,
      theme: resolvedTheme() === "dark" ? "dark" : "default",
      securityLevel: "strict"
    });
    diagrams.forEach(function (el) {
      el.removeAttribute("data-processed");
      el.textContent = el.getAttribute("data-src");
    });
    window.mermaid.run({ nodes: diagrams }).catch(function (e) { console.error("Mermaid:", e); });
  }

  if (diagrams.length) {
    var script = doc.createElement("script");
    script.src = root + "assets/mermaid.min.js";
    script.onload = renderMermaid;
    script.onerror = function () { console.error("Could not load " + script.src); };
    doc.head.appendChild(script);
    if (window.matchMedia) {
      var mq = window.matchMedia("(prefers-color-scheme: dark)");
      var onOsChange = function () { if (!html.getAttribute("data-theme")) { renderMermaid(); } };
      if (mq.addEventListener) { mq.addEventListener("change", onOsChange); }
    }
  }
})();
