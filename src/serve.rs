//! `dokt serve`: build, serve on localhost, rebuild when files change,
//! and reload open browser tabs.
//!
//! Change detection is a once-a-second re-crawl comparing file sizes and
//! modification times. The crawl prunes excluded directories, so it never
//! touches node_modules, and it needs no OS-level watch handles.

use crate::config::{self, Loaded};
use crate::crawl;
use crate::markdown::percent_decode;
use crate::site;
use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use std::{fs, thread};
use tiny_http::{Header, Request, Response, Server};

/// Added to every HTML page the dev server sends: polls `/__version` once a second
/// and reloads the page when it changes.
const LIVE_RELOAD: &str = "<script>(function(){var v=null;function t(){fetch('/__version',{cache:'no-store'}).then(function(r){return r.text()}).then(function(x){if(v===null){v=x}else if(v!==x){location.reload();return}setTimeout(t,1000)}).catch(function(){setTimeout(t,2000)})}t()})();</script>";

/// Build the site, serve it, and keep it up to date until the process is stopped.
///
/// A background thread re-crawls once a second and rebuilds when something
/// changed. Four worker threads answer HTTP requests.
pub fn serve(loaded: Loaded, out_override: Option<PathBuf>, host: &str, port: u16) -> Result<()> {
    let root = loaded.root.clone();
    let mut cfg = loaded.config.clone();
    let out_dir = site::out_dir_for(&root, &cfg, out_override.as_deref());

    let first = crawl::crawl(&root, &cfg, &out_dir)?;
    let report = site::build(&root, &cfg, &out_dir, &first)?;
    site::print_report(&report, &out_dir);

    let server = Server::http((host, port)).map_err(|e| anyhow!("cannot listen on {host}:{port}: {e}"))?;
    let server = Arc::new(server);
    let version = Arc::new(AtomicU64::new(1));

    // ---- watcher ----
    {
        let version = Arc::clone(&version);
        let root = root.clone();
        let out_dir = out_dir.clone();
        let config_path = loaded.path.clone();
        let mut last = first.fingerprint();
        let mut last_cfg = mtime(&config_path);
        // Only report a failure when it changes, not once a second.
        let mut last_error: Option<String> = None;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(1000));
            let mut force = false;
            let now_cfg = mtime(&config_path);
            if now_cfg != last_cfg {
                last_cfg = now_cfg;
                match config::load(Some(&config_path)) {
                    Ok(l) => {
                        cfg = l.config;
                        force = true;
                        eprintln!("config reloaded (changes to out_dir apply after a restart)");
                    }
                    Err(e) => eprintln!("config error, keeping previous settings: {e:#}"),
                }
            }
            let crawled = match crawl::crawl(&root, &cfg, &out_dir) {
                Ok(c) => {
                    if last_error.take().is_some() {
                        eprintln!("problem fixed, rebuilding");
                        force = true;
                    }
                    c
                }
                Err(e) => {
                    let msg = format!("{e:#}");
                    if last_error.as_deref() != Some(msg.as_str()) {
                        eprintln!("crawl failed (serving the last good build): {msg}");
                        last_error = Some(msg);
                    }
                    continue;
                }
            };
            let fp = crawled.fingerprint();
            if force || fp != last {
                last = fp;
                match site::build(&root, &cfg, &out_dir, &crawled) {
                    Ok(r) => {
                        site::print_report(&r, &out_dir);
                        version.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(e) => eprintln!("build failed: {e:#}"),
                }
            }
        });
    }

    println!("Serving http://{host}:{port}/  (Ctrl-C to stop)");

    let mut workers = Vec::new();
    for _ in 0..4 {
        let server = Arc::clone(&server);
        let version = Arc::clone(&version);
        let out_dir = out_dir.clone();
        workers.push(thread::spawn(move || {
            for request in server.incoming_requests() {
                handle(request, &out_dir, &version);
            }
        }));
    }
    for w in workers {
        let _ = w.join();
    }
    Ok(())
}

/// A file's modification time, if it can be read.
fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Build an HTTP header from a name and value that are known to be valid.
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

/// Answer one request: the version probe, a file from the output directory, or a 404.
///
/// The request path is checked component by component, so nothing outside the
/// output directory can be reached.
fn handle(req: Request, out_dir: &Path, version: &AtomicU64) {
    let url = req.url().to_string();
    let path_only = url.split(['?', '#']).next().unwrap_or("/");

    if path_only == "/__version" {
        let resp = Response::from_string(version.load(Ordering::SeqCst).to_string())
            .with_header(header("Content-Type", "text/plain"))
            .with_header(header("Cache-Control", "no-store"));
        let _ = req.respond(resp);
        return;
    }

    let decoded = percent_decode(path_only);
    let mut target = out_dir.to_path_buf();
    for comp in decoded.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." || comp.contains('\\') || comp.contains(':') || comp.contains('\0') {
            let _ = req.respond(Response::from_string("bad request").with_status_code(400));
            return;
        }
        target.push(comp);
    }
    if target.is_dir() {
        target.push("index.html");
    }

    match fs::read(&target) {
        Ok(bytes) => {
            let ext = target.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            let body = if ext == "html" {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let injected = match text.rfind("</body>") {
                    Some(i) => format!("{}{}{}", &text[..i], LIVE_RELOAD, &text[i..]),
                    None => format!("{text}{LIVE_RELOAD}"),
                };
                injected.into_bytes()
            } else {
                bytes
            };
            let resp = Response::from_data(body)
                .with_header(header("Content-Type", content_type(&ext)))
                .with_header(header("Cache-Control", "no-store"));
            let _ = req.respond(resp);
        }
        Err(_) => {
            let html = "<!doctype html><meta charset=utf-8><title>Not found</title>\
                        <p>Not found. <a href=\"/\">Back to the docs home</a>.</p>";
            let resp = Response::from_string(html)
                .with_status_code(404)
                .with_header(header("Content-Type", "text/html; charset=utf-8"));
            let _ = req.respond(resp);
        }
    }
}

/// Content type for a file extension.
fn content_type(ext: &str) -> &'static str {
    match ext {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "pdf" => "application/pdf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
