//! Printing a web page to PDF with an installed Chrome, Chromium or Edge.
//!
//! The browser is started headless with a remote-debugging port, and driven
//! over the Chrome DevTools Protocol (CDP) on a WebSocket. Nothing is
//! downloaded or bundled: this uses whatever browser is already on the machine.
//!
//! Why a browser at all? Mermaid diagrams are drawn by JavaScript, so
//! producing the same output as the website needs a real browser. Driving it
//! over CDP, rather than using `--print-to-pdf`, lets us wait until diagrams
//! and images have finished loading, and ask for page numbers and PDF bookmarks.

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// How long to wait for the browser to start, and for the page to finish loading.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the page may take to finish drawing diagrams and loading images.
const READY_TIMEOUT: Duration = Duration::from_secs(120);

/// What to print and how to decorate each page.
pub struct PdfJob<'a> {
    /// `file://` URL of the document to print.
    pub url: &'a str,
    /// HTML for the running header and footer. The browser fills elements with
    /// the classes `pageNumber` and `totalPages`.
    pub header_html: &'a str,
    pub footer_html: &'a str,
}

/// The printed PDF.
pub struct PdfResult {
    pub bytes: Vec<u8>,
    /// Mermaid diagrams that failed to draw (their source text is printed instead).
    pub diagram_errors: usize,
}

// ---------------------------------------------------------------------------
// Finding a browser
// ---------------------------------------------------------------------------

/// Executable names to look for on `PATH`, in order of preference.
const BROWSER_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    "microsoft-edge",
    "microsoft-edge-stable",
    "msedge",
];

/// Well-known install locations that are usually not on `PATH`.
const BROWSER_PATHS: &[&str] = &[
    // macOS
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    // Windows
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    // Linux
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/snap/bin/chromium",
];

/// Locate a browser: an explicit path (option or config) first, then the
/// `DOKT_BROWSER` environment variable, then `PATH`, then common install paths.
pub fn find_browser(explicit: Option<&str>) -> Result<PathBuf> {
    let chosen = explicit
        .map(str::to_string)
        .or_else(|| std::env::var("DOKT_BROWSER").ok())
        .filter(|s| !s.trim().is_empty());
    if let Some(p) = chosen {
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
        bail!("the browser `{p}` does not exist or is not a file");
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in BROWSER_NAMES {
                for candidate in [dir.join(name), dir.join(format!("{name}.exe"))] {
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
    }
    for p in BROWSER_PATHS {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!(
        "no Chrome, Chromium or Edge found. PDF export prints through a browser; install one, \
         or point to it with --browser <path>, `browser` under [export] in the config, or the \
         DOKT_BROWSER environment variable"
    )
}

// ---------------------------------------------------------------------------
// Running the browser
// ---------------------------------------------------------------------------

/// A running headless browser. Dropping it kills the process and removes its
/// temporary profile directory, so nothing is left behind on any error path.
struct Browser {
    child: Child,
    profile: PathBuf,
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

/// Start the browser and return it with its DevTools WebSocket URL.
fn launch(browser: &Path, no_sandbox: bool) -> Result<(Browser, String)> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let profile = std::env::temp_dir().join(format!("dokt-browser-{}-{nanos}", std::process::id()));

    let mut cmd = Command::new(browser);
    cmd.args([
        "--headless",
        "--disable-gpu",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--hide-scrollbars",
        "--mute-audio",
        // Port 0: the browser picks a free port and reports it on stderr.
        "--remote-debugging-port=0",
    ]);
    cmd.arg(format!("--user-data-dir={}", profile.display()));
    if no_sandbox {
        cmd.arg("--no-sandbox");
    }
    cmd.arg("about:blank").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = cmd.spawn().with_context(|| format!("could not start {}", browser.display()))?;
    let stderr = child.stderr.take().expect("stderr was requested as a pipe");
    let browser = Browser { child, profile };

    // The browser announces "DevTools listening on ws://..." on stderr. Keep
    // reading afterwards so it never blocks on a full pipe, and remember the
    // last few lines to explain a failed start.
    let (tx, rx) = mpsc::channel::<String>();
    let tail: Arc<Mutex<Vec<String>>> = Arc::default();
    let tail_writer = Arc::clone(&tail);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            if let Some(url) = line.split("DevTools listening on ").nth(1) {
                let _ = tx.send(url.trim().to_string());
            }
            let mut t = tail_writer.lock().unwrap();
            t.push(line);
            if t.len() > 6 {
                t.remove(0);
            }
        }
    });

    let mut browser = browser;
    let started = Instant::now();
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(url) => return Ok((browser, url)),
            Err(_) => {
                if let Some(status) = browser.child.try_wait()? {
                    let last = tail.lock().unwrap().join("\n  ");
                    bail!(
                        "the browser exited right away ({status}). If this is a container or you are \
                         running as root, try --no-sandbox.\n  {last}"
                    );
                }
                if started.elapsed() > START_TIMEOUT {
                    bail!("the browser did not start within {} seconds", START_TIMEOUT.as_secs());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// DevTools Protocol client
// ---------------------------------------------------------------------------

/// A minimal synchronous CDP client: send a command, wait for its reply, and
/// ignore the browser's event notifications.
struct Cdp {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    next_id: u64,
}

impl Cdp {
    /// Open the WebSocket to the browser's DevTools endpoint.
    fn connect(url: &str) -> Result<Cdp> {
        // PDFs can be large, so lift the default message size limits.
        let config = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
        let (mut ws, _) = tungstenite::client::connect_with_config(url, Some(config), 0)
            .with_context(|| format!("could not connect to the browser at {url}"))?;
        // A hung browser must not hang us forever.
        if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
            stream.set_read_timeout(Some(READY_TIMEOUT))?;
        }
        Ok(Cdp { ws, next_id: 0 })
    }

    /// Run one command (optionally inside a page `session`) and return its `result`.
    fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let mut msg = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(Message::text(msg.to_string()))?;

        loop {
            let incoming = self
                .ws
                .read()
                .map_err(|e| anyhow!("lost contact with the browser during {method}: {e}"))?;
            let Message::Text(text) = incoming else { continue };
            let reply: Value = serde_json::from_str(text.as_str())?;
            if reply["id"].as_u64() != Some(id) {
                continue; // an event, or a reply to something else
            }
            if let Some(err) = reply.get("error") {
                bail!("{method} failed: {}", err["message"].as_str().unwrap_or("unknown error"));
            }
            return Ok(reply["result"].clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Printing
// ---------------------------------------------------------------------------

/// Open `job.url`, wait until the page reports it is ready, and print it to PDF.
///
/// The page signals readiness by setting `data-ready="1"` on `<body>` once its
/// diagrams are drawn and its images loaded (see `assets/print.js`).
pub fn print_to_pdf(browser: &Path, no_sandbox: bool, job: &PdfJob) -> Result<PdfResult> {
    let (_running, ws_url) = launch(browser, no_sandbox)?;
    let mut cdp = Cdp::connect(&ws_url)?;

    let target = cdp.call("Target.createTarget", json!({ "url": "about:blank" }), None)?;
    let target_id = target["targetId"].as_str().ok_or_else(|| anyhow!("the browser returned no target"))?;
    let attached = cdp.call("Target.attachToTarget", json!({ "targetId": target_id, "flatten": true }), None)?;
    let session = attached["sessionId"].as_str().ok_or_else(|| anyhow!("the browser returned no session"))?.to_string();
    let s = Some(session.as_str());

    cdp.call("Page.enable", json!({}), s)?;
    let nav = cdp.call("Page.navigate", json!({ "url": job.url }), s)?;
    if let Some(err) = nav["errorText"].as_str() {
        bail!("the browser could not open the export document: {err}");
    }

    // Poll until print.js says the page is ready.
    let started = Instant::now();
    let diagram_errors = loop {
        let probe = cdp.call(
            "Runtime.evaluate",
            json!({
                "expression": "document.body && document.body.dataset.ready === '1' \
                               ? 'ready:' + (document.body.dataset.diagramErrors || '0') : 'wait'",
                "returnByValue": true,
            }),
            s,
        )?;
        let value = probe["result"]["value"].as_str().unwrap_or("wait");
        if let Some(n) = value.strip_prefix("ready:") {
            break n.parse().unwrap_or(0);
        }
        if started.elapsed() > READY_TIMEOUT {
            bail!("the document did not finish loading within {} seconds", READY_TIMEOUT.as_secs());
        }
        std::thread::sleep(Duration::from_millis(150));
    };

    let mut params = json!({
        "printBackground": true,
        // Paper size and margins come from the document's own @page rule.
        "preferCSSPageSize": true,
        "displayHeaderFooter": true,
        "headerTemplate": job.header_html,
        "footerTemplate": job.footer_html,
        // Stream the result back in chunks instead of one giant message.
        "transferMode": "ReturnAsStream",
        // PDF bookmarks from the headings, and an accessible (tagged) PDF.
        "generateDocumentOutline": true,
        "generateTaggedPDF": true,
    });
    let printed = match cdp.call("Page.printToPDF", params.clone(), s) {
        Ok(v) => v,
        Err(_) => {
            // Older browsers do not know the two newest options; retry without them.
            params.as_object_mut().unwrap().remove("generateDocumentOutline");
            params.as_object_mut().unwrap().remove("generateTaggedPDF");
            cdp.call("Page.printToPDF", params, s)?
        }
    };
    let handle = printed["stream"].as_str().ok_or_else(|| anyhow!("the browser returned no PDF stream"))?.to_string();

    // The stream belongs to the page, so it must be read through the page's session.
    let mut bytes = Vec::new();
    loop {
        let chunk = cdp.call("IO.read", json!({ "handle": handle, "size": 1 << 20 }), s)?;
        let data = chunk["data"].as_str().unwrap_or("");
        if chunk["base64Encoded"].as_bool().unwrap_or(false) {
            bytes.extend(base64::engine::general_purpose::STANDARD.decode(data)?);
        } else {
            bytes.extend(data.as_bytes());
        }
        if chunk["eof"].as_bool().unwrap_or(true) {
            break;
        }
    }
    let _ = cdp.call("IO.close", json!({ "handle": handle }), s);
    let _ = cdp.call("Browser.close", json!({}), None);

    if !bytes.starts_with(b"%PDF") {
        bail!("the browser returned something that is not a PDF");
    }
    Ok(PdfResult { bytes, diagram_errors })
}
