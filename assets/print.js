// Runs inside the browser while dokt prints the PDF. It draws the Mermaid
// diagrams and waits for images, then sets <body data-ready="1"> so dokt knows
// the document is ready to print. If anything fails, it still reports ready,
// with the number of diagrams that could not be drawn.
(function () {
  "use strict";
  var body = document.body;
  var failed = 0;

  // A PDF cannot be expanded by clicking, so show the contents of every <details>.
  Array.prototype.forEach.call(document.querySelectorAll("details"), function (d) { d.open = true; });

  // Resolves when every image has loaded (or failed, so one bad image cannot hang the export).
  function imagesReady() {
    return Promise.all(Array.prototype.map.call(document.images, function (img) {
      if (img.complete) { return null; }
      return new Promise(function (done) {
        img.addEventListener("load", done);
        img.addEventListener("error", done);
      });
    }));
  }

  // Loads the bundled Mermaid and draws every diagram, always in the light theme.
  // Each diagram is checked first: Mermaid draws an error picture for invalid
  // syntax instead of failing, which would silently put it into the PDF. Invalid
  // diagrams are left as their source text (styled by .mermaid-failed) and counted.
  function drawDiagrams() {
    var nodes = Array.prototype.slice.call(document.querySelectorAll("pre.mermaid"));
    if (!nodes.length) { return Promise.resolve(); }
    return new Promise(function (done) {
      var script = document.createElement("script");
      script.src = "assets/mermaid.min.js";
      script.onerror = function () { failed = nodes.length; done(); };
      script.onload = function () {
        var m = window.mermaid;
        m.initialize({ startOnLoad: false, theme: "default", securityLevel: "strict" });
        Promise.all(nodes.map(function (n) {
          return m.parse(n.textContent, { suppressErrors: true }).catch(function () { return false; });
        }))
          .then(function (valid) {
            var good = [];
            nodes.forEach(function (n, i) {
              if (valid[i]) { good.push(n); } else { failed++; n.classList.add("mermaid-failed"); }
            });
            return m.run({ nodes: good, suppressErrors: true });
          })
          .catch(function () { /* whatever did not draw is counted below */ })
          .then(function () {
            nodes.forEach(function (n) {
              if (!n.classList.contains("mermaid-failed") && !n.querySelector("svg")) {
                failed++;
                n.classList.add("mermaid-failed");
              }
            });
            done();
          });
      };
      document.head.appendChild(script);
    });
  }

  function finish() {
    body.setAttribute("data-diagram-errors", String(failed));
    body.setAttribute("data-ready", "1");
  }

  Promise.all([drawDiagrams(), imagesReady()])
    .then(function () { return document.fonts ? document.fonts.ready : null; })
    .then(finish, finish);
})();
