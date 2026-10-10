// `skini serve` (also the default when double-clicked): runs the local bridge the
// website talks to — on 127.0.0.1:7879 — so the whole UI stays in the browser and
// there's nothing to install but this one self-contained exe.
//
// STABILITY DESIGN:
//  * Every device operation (list / scan / apply) runs in a SEPARATE short-lived
//    child process (this same exe in CLI mode). If the USB/device code crashes or
//    the iPhone drops mid-operation, only the child dies — the bridge stays up and
//    returns an error. This is what keeps the "companion" from turning off.
//  * Requests are served by a small pool of worker threads, so checking the
//    connection (ping) never blocks while a 25s card-scan is running.
//  * Each request is wrapped in catch_unwind, so a panic can't take down the server.
//
// The browser (same origin as our server) fetches the licensed skin bytes itself
// and hands them here as base64; this process only does the device work.

use std::io::{BufRead, BufReader};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn simulate() -> bool {
    matches!(std::env::var("SKINI_SIMULATE").as_deref(), Ok("1") | Ok("true"))
}

fn allowed_origins() -> Vec<String> {
    std::env::var("SKINI_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "https://www.applepay.skin,https://applepay.skin,https://skini-production.up.railway.app,http://localhost:8080,http://127.0.0.1:8080".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn site_url() -> String {
    // Dev default = the local server. When you deploy to your own domain, set the
    // SKINI_SITE env var (or change this) to e.g. https://skini.app/app.
    std::env::var("SKINI_SITE").unwrap_or_else(|_| "https://www.applepay.skin/app".to_string())
}

fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("skini.exe"))
}

// The skini:// link and the autostart entry must point at ONE stable copy, not at whatever file
// was double-clicked (Downloads\skini (4).exe ...), or an old download keeps getting relaunched.
// Every start copies the running build to %LOCALAPPDATA%\Skini\skini.exe when it differs.
// On macOS the app bundle in /Applications already is that stable copy.
#[cfg(windows)]
fn installed_path() -> PathBuf {
    crate::platform::app_data_dir().join("Skini").join("skini.exe")
}

#[cfg(windows)]
fn install_self() -> PathBuf {
    let me = exe_path();
    let target = installed_path();
    if me == target { return target; }
    let same = match (std::fs::read(&me), std::fs::read(&target)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same {
        let _ = std::fs::create_dir_all(target.parent().unwrap());
        if std::fs::copy(&me, &target).is_err() { return me; } // stable copy busy: fall back to this file
    }
    target
}

// Run this same binary in CLI mode (list/scan/apply) as an isolated child and
// parse its final JSON line. A crash in the child becomes an error here, not a
// dead bridge.
fn run_self(args: &[&str]) -> Value {
    match Command::new(exe_path()).args(args).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let last = stdout.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("");
            match serde_json::from_str::<Value>(last) {
                Ok(v) => v,
                Err(_) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let msg = if out.status.success() { "לא התקבלה תשובה מהמכשיר" } else { err.trim() };
                    json!({ "ok": false, "error": if msg.is_empty() { "פעולת המכשיר נכשלה" } else { msg } })
                }
            }
        }
        Err(e) => json!({ "ok": false, "error": format!("לא ניתן להפעיל את רכיב המכשיר: {e}") }),
    }
}

/* ─────────────── Device operations (isolated) ─────────────── */
fn devices_json() -> Vec<Value> {
    if simulate() {
        return vec![json!({ "id": "sim-iphone", "name": "iPhone (סימולציה)", "connected": true })];
    }
    let r = run_self(&["list"]);
    r.get("devices")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .map(|d| {
                    let name = d.get("name").and_then(|v| v.as_str()).unwrap_or("iPhone");
                    let ios = d.get("ios_version").and_then(|v| v.as_str()).unwrap_or("");
                    json!({
                        "id": d.get("udid").and_then(|v| v.as_str()).unwrap_or(""),
                        "name": if ios.is_empty() { name.to_string() } else { format!("{name} · iOS {ios}") },
                        "connected": true,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn do_detect() -> Value {
    if simulate() {
        std::thread::sleep(Duration::from_millis(500));
        return json!({ "ok": true, "cards": [{ "hash": "SIMULATED-CARD", "name": "כרטיס לדוגמה" }] });
    }
    run_self(&["scan", "--seconds", "25"])
}

/* ─────────────── Live card scan (background job) ─────────────── */
// One scan child at a time. It streams a JSON line per card; a reader thread
// collects them so the browser can poll /skini/scan/status and show cards live.
struct ScanState {
    running: bool,
    cards: Vec<Value>,
    error: Option<String>,
    child: Option<Child>,
    started: Option<Instant>,
}

static SCAN: OnceLock<Mutex<ScanState>> = OnceLock::new();

fn scan_lock() -> MutexGuard<'static, ScanState> {
    SCAN.get_or_init(|| Mutex::new(ScanState { running: false, cards: vec![], error: None, child: None, started: None }))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn scan_status_json(st: &ScanState) -> Value {
    json!({
        "ok": true,
        "running": st.running,
        "cards": st.cards,
        "error": st.error,
        "elapsed": st.started.map(|t| t.elapsed().as_secs()).unwrap_or(0),
    })
}

fn scan_start() -> Value {
    if simulate() {
        let mut st = scan_lock();
        st.running = false;
        st.error = None;
        st.cards = vec![json!({ "hash": "SIMULATED-CARD", "name": "כרטיס לדוגמה" })];
        return scan_status_json(&st);
    }
    let mut st = scan_lock();
    if st.running {
        return scan_status_json(&st);
    }
    let spawned = Command::new(exe_path())
        .args(["scan", "--seconds", "180", "--stream", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            let stdout = child.stdout.take();
            st.running = true;
            st.cards.clear();
            st.error = None;
            st.child = Some(child);
            st.started = Some(Instant::now());
            let snapshot = scan_status_json(&st);
            drop(st);
            std::thread::spawn(move || {
                if let Some(out) = stdout {
                    for line in BufReader::new(out).lines().flatten() {
                        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                        let mut st = scan_lock();
                        if v.get("event").and_then(|e| e.as_str()) == Some("card") {
                            let hash = v.get("hash").and_then(|h| h.as_str()).unwrap_or("");
                            if !hash.is_empty() && !st.cards.iter().any(|c| c.get("hash").and_then(|h| h.as_str()) == Some(hash)) {
                                st.cards.push(json!({ "hash": hash, "name": v.get("name").cloned().unwrap_or(json!("")) }));
                            }
                        } else if v.get("ok").and_then(|o| o.as_bool()) == Some(false) {
                            st.error = v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string());
                        }
                    }
                }
                let mut st = scan_lock();
                if let Some(mut c) = st.child.take() {
                    let _ = c.wait();
                }
                st.running = false;
            });
            snapshot
        }
        Err(e) => json!({ "ok": false, "error": format!("לא ניתן להפעיל האזנה: {e}") }),
    }
}

fn scan_stop() -> Value {
    let mut st = scan_lock();
    if let Some(c) = st.child.as_mut() {
        let _ = c.kill();
    }
    scan_status_json(&st)
}

fn do_apply(body: &Value) -> Value {
    let skin_id = body.get("skinId").and_then(|v| v.as_str()).unwrap_or("");
    let card = body.get("cardHash").and_then(|v| v.as_str()).unwrap_or("");
    let image_b64 = body.get("imageB64").and_then(|v| v.as_str()).unwrap_or("");

    if simulate() {
        std::thread::sleep(Duration::from_millis(1000));
        return json!({ "ok": true, "message": format!("הסקין {skin_id} הוחל (סימולציה).") });
    }
    if card.is_empty() {
        return json!({ "ok": false, "error": "חסר מזהה כרטיס (בצעו \"זיהוי כרטיס\" קודם)" });
    }
    if image_b64.is_empty() {
        return json!({ "ok": false, "error": "חסרים נתוני תמונה" });
    }
    let bytes = match base64::engine::general_purpose::STANDARD.decode(image_b64) {
        Ok(b) => b,
        Err(_) => return json!({ "ok": false, "error": "נתוני תמונה לא תקינים" }),
    };
    // Write the image to a temp file for the isolated apply child.
    let tmp = std::env::temp_dir().join(format!("skini-{}.img", std::process::id()));
    if std::fs::write(&tmp, &bytes).is_err() {
        return json!({ "ok": false, "error": "לא ניתן לשמור קובץ זמני" });
    }
    let tmp_str = tmp.to_string_lossy().to_string();
    let result = run_self(&["apply", "--image", &tmp_str, "--card", card]);
    let _ = std::fs::remove_file(&tmp);
    result
}

/* ─────────────── HTTP ─────────────── */
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn cors_origin(req: &Request) -> Option<String> {
    let origin = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().to_string())?;
    allowed_origins().contains(&origin).then_some(origin)
}

fn respond(req: Request, status: u16, body: Value, origin: Option<String>) {
    let mut resp = Response::from_string(body.to_string()).with_status_code(status);
    resp.add_header(header("Content-Type", "application/json; charset=utf-8"));
    if let Some(o) = origin {
        resp.add_header(header("Access-Control-Allow-Origin", &o));
        resp.add_header(header("Vary", "Origin"));
    }
    let _ = req.respond(resp);
}

fn read_body(req: &mut Request) -> Value {
    use std::io::Read;
    let mut s = String::new();
    let _ = req.as_reader().read_to_string(&mut s);
    serde_json::from_str(&s).unwrap_or(json!({}))
}

fn route(method: &Method, path: &str, req: &mut Request) -> (u16, Value) {
    match (method, path) {
        (Method::Get, "/skini/ping") => (
            200,
            json!({ "app": "skini-desktop", "version": VERSION, "backend": if simulate() { "simulation" } else { "device" }, "requiresCard": !simulate(), "devices": devices_json() }),
        ),
        (Method::Post, "/skini/detect") => (200, do_detect()),
        (Method::Post, "/skini/restore") => {
            let b = read_body(req);
            let card = b.get("cardHash").and_then(|v| v.as_str()).unwrap_or("");
            let body = if simulate() {
                json!({ "ok": true, "message": "העיצוב המקורי הוחזר (סימולציה)." })
            } else if card.is_empty() {
                json!({ "ok": false, "error": "חסר מזהה כרטיס" })
            } else {
                run_self(&["restore", "--card", card])
            };
            let status = if body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) { 200 } else { 500 };
            (status, body)
        }
        (Method::Get, "/skini/autostart") => (200, json!({ "ok": true, "enabled": autostart_enabled() })),
        (Method::Post, "/skini/autostart") => {
            let b = read_body(req);
            let on = b.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            let ok = set_autostart(on);
            (if ok { 200 } else { 500 }, json!({ "ok": ok, "enabled": autostart_enabled() }))
        }
        (Method::Post, "/skini/scan/start") => (200, scan_start()),
        (Method::Get, "/skini/scan/status") => (200, scan_status_json(&scan_lock())),
        (Method::Post, "/skini/scan/stop") => (200, scan_stop()),
        (Method::Post, "/skini/apply") => {
            let b = read_body(req);
            let body = do_apply(&b);
            let status = if body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) { 200 } else { 500 };
            (status, body)
        }
        _ => (404, json!({ "error": "not found" })),
    }
}

fn handle(mut req: Request) {
    let origin = cors_origin(&req);
    if req.method() == &Method::Options {
        let mut resp = Response::empty(204);
        if let Some(o) = &origin {
            resp.add_header(header("Access-Control-Allow-Origin", o));
            resp.add_header(header("Access-Control-Allow-Headers", "Content-Type"));
            resp.add_header(header("Access-Control-Allow-Methods", "GET, POST, OPTIONS"));
            // Chrome's Private Network Access: an https site may call this local bridge only if it opts in.
            resp.add_header(header("Access-Control-Allow-Private-Network", "true"));
        }
        let _ = req.respond(resp);
        return;
    }

    let method = req.method().clone();
    let path = req.url().split('?').next().unwrap_or("").to_string();

    // A panic in a handler must not kill the server.
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| route(&method, &path, &mut req)));
    let (status, body) = result.unwrap_or_else(|_| (500, json!({ "ok": false, "error": "שגיאה פנימית" })));
    respond(req, status, body, origin);
}

fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(not(windows))]
    let _ = Command::new("open").arg(url).spawn();
}

/* ─────────────── "Open from the website" + autostart (Windows registry) ─────────────── */
// Registers the skini:// URL scheme so the website's "Open Skini" button can launch
// this exe, and optionally adds it to the user's startup. HKCU only, so no admin needed.
#[cfg(windows)]
fn reg(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    Command::new("reg")
        .args(args)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn register_protocol() {
    let exe = install_self();
    // keep an existing autostart entry pointing at the stable copy too
    if autostart_enabled() { set_autostart(true); }
    let cmd = format!("\"{}\" serve --quiet \"%1\"", exe.to_string_lossy());
    let base = r"HKCU\Software\Classes\skini";
    reg(&["add", base, "/ve", "/d", "URL:Skini Protocol", "/f"]);
    reg(&["add", base, "/v", "URL Protocol", "/d", "", "/f"]);
    reg(&["add", &format!(r"{base}\shell\open\command"), "/ve", "/d", &cmd, "/f"]);
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
fn autostart_enabled() -> bool {
    reg(&["query", RUN_KEY, "/v", "Skini"])
}

#[cfg(windows)]
fn set_autostart(on: bool) -> bool {
    if on {
        let cmd = format!("\"{}\" serve --quiet", installed_path().to_string_lossy());
        reg(&["add", RUN_KEY, "/v", "Skini", "/d", &cmd, "/f"])
    } else {
        !autostart_enabled() || reg(&["delete", RUN_KEY, "/v", "Skini", "/f"])
    }
}

/* ─────────────── macOS: skini:// comes from Info.plist, autostart is a LaunchAgent ─────────────── */
// The URL scheme is declared in Skini.app/Contents/Info.plist (CFBundleURLTypes), so macOS
// registers it by itself the first time the app runs. Nothing to write here.
#[cfg(target_os = "macos")]
fn register_protocol() {
    if autostart_enabled() { set_autostart(true); } // keep the agent pointing at the current app
}

#[cfg(target_os = "macos")]
fn launch_agent_path() -> PathBuf {
    crate::platform::home_dir().join("Library").join("LaunchAgents").join("skin.applepay.skini.plist")
}

#[cfg(target_os = "macos")]
fn autostart_enabled() -> bool {
    launch_agent_path().is_file()
}

#[cfg(target_os = "macos")]
fn set_autostart(on: bool) -> bool {
    let path = launch_agent_path();
    if !on {
        return !path.exists() || std::fs::remove_file(&path).is_ok();
    }
    let exe = exe_path().to_string_lossy().replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>skin.applepay.skini</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>serve</string><string>--quiet</string></array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#
    );
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    std::fs::write(&path, plist).is_ok()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn register_protocol() {}
#[cfg(not(any(windows, target_os = "macos")))]
fn autostart_enabled() -> bool { false }
#[cfg(not(any(windows, target_os = "macos")))]
fn set_autostart(_on: bool) -> bool { false }

pub fn run(open: bool, quiet: bool) -> i32 {
    let addr = "127.0.0.1:7879";
    let server = match Server::http(addr) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            // Started with --quiet and already running: nothing to do.
            if quiet {
                return 0;
            }
            rfd::MessageDialog::new()
                .set_title("Skini")
                .set_level(rfd::MessageLevel::Error)
                .set_description(format!(
                    "לא ניתן להפעיל את Skini על {addr}.\nייתכן ש-Skini כבר פועל ברקע, או שהפורט 7879 תפוס.\n\n({e})"
                ))
                .show();
            return 1;
        }
    };
    let backend = if simulate() { "simulation" } else { "device" };
    eprintln!("Skini bridge v{VERSION} on http://{addr} (backend: {backend})");

    // Make sure the website can launch us next time (cheap, done off the main path).
    std::thread::spawn(register_protocol);

    // Worker pool so ping isn't blocked while a scan/apply child is running.
    let mut workers = Vec::new();
    for _ in 0..4 {
        let s = server.clone();
        workers.push(std::thread::spawn(move || loop {
            match s.recv() {
                Ok(req) => handle(req),
                Err(_) => break,
            }
        }));
    }

    if open {
        open_browser(&site_url());
    }
    for w in workers {
        let _ = w.join();
    }
    0
}
