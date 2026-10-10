// Headless CLI for the Skini companion to drive. Reuses the existing device /
// flasher / image modules — no GUI. All machine-readable output goes to STDOUT
// as a single JSON object; human progress/log goes to STDERR.
//
// Commands:
//   skini list
//       -> {"ok":true,"devices":[{udid,name,product_type,ios_version,build,transports}]}
//   skini scan [--udid U] [--seconds N]
//       -> {"ok":true,"cards":[{hash,name}]}
//   skini apply --image <path> --card <hash> [--udid U] [--mode auto|usb|wifi]
//       -> {"ok":true,"message":"..."}
//   skini restore --card <hash> [--udid U] [--mode ...]
//       -> {"ok":true,"message":"..."}

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde_json::json;

use crate::device::{list_connected_devices, ConnectionMode};
use crate::flasher::{flash_wallet_skin, restore_wallet_original};
use crate::image_skin::PreparedSkin;
use crate::scanner::scan_syslog_for_cards;

pub fn is_cli(args: &[String]) -> bool {
    matches!(args.first().map(|s| s.as_str()), Some("list" | "scan" | "apply" | "restore" | "--help" | "-h" | "version"))
}

fn parse_opts(rest: &[String]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(key) = a.strip_prefix("--") {
            let val = rest.get(i + 1).cloned().unwrap_or_default();
            m.insert(key.to_string(), val);
            i += 2;
        } else {
            i += 1;
        }
    }
    m
}

fn mode_from(opts: &HashMap<String, String>) -> ConnectionMode {
    match opts.get("mode").map(|s| s.as_str()) {
        Some("usb") => ConnectionMode::Usb,
        Some("wifi") => ConnectionMode::Wifi,
        _ => ConnectionMode::Auto,
    }
}

// Pick the target device: explicit --udid, else the only/first connected one.
// The device briefly re-enumerates after a previous session closes, so retry a few
// times before giving up — this keeps "apply" working right after "detect".
fn resolve_udid(opts: &HashMap<String, String>) -> Result<String> {
    if let Some(u) = opts.get("udid") {
        if !u.is_empty() {
            return Ok(u.clone());
        }
    }
    for attempt in 0..8 {
        if let Ok(devices) = list_connected_devices() {
            if let Some(d) = devices.into_iter().next() {
                return Ok(d.udid);
            }
        }
        if attempt < 7 {
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
    }
    Err(anyhow!("No iPhone detected. Connect it by cable and tap 'Trust This Computer'."))
}

fn emit_ok(value: serde_json::Value) {
    println!("{}", value);
}

fn emit_err(msg: &str) {
    println!("{}", json!({ "ok": false, "error": msg }));
}

// Returns process exit code.
pub fn run(args: &[String]) -> i32 {
    let cmd = args[0].as_str();
    let opts = parse_opts(&args[1..]);
    let result = match cmd {
        "version" | "--help" | "-h" => {
            emit_ok(json!({ "ok": true, "app": "skini-desktop", "version": env!("CARGO_PKG_VERSION") }));
            return 0;
        }
        "list" => cmd_list(),
        "scan" => cmd_scan(&opts),
        "apply" => cmd_apply(&opts),
        "restore" => cmd_restore(&opts),
        other => Err(anyhow!("Unknown command: {other}")),
    };

    match result {
        Ok(value) => {
            emit_ok(value);
            0
        }
        Err(e) => {
            emit_err(&format!("{e:#}"));
            1
        }
    }
}

fn cmd_list() -> Result<serde_json::Value> {
    let devices = list_connected_devices().context("Could not list devices")?;
    let arr: Vec<_> = devices
        .iter()
        .map(|d| {
            json!({
                "udid": d.udid,
                "name": d.name,
                "product_type": d.product_type,
                "ios_version": d.ios_version,
                "build": d.build_version,
                "transports": d.transports.iter().map(|t| t.label()).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "ok": true, "devices": arr }))
}

fn cmd_scan(opts: &HashMap<String, String>) -> Result<serde_json::Value> {
    let seconds: u64 = opts.get("seconds").and_then(|s| s.parse().ok()).unwrap_or(20);
    let udid = opts.get("udid").filter(|s| !s.is_empty()).cloned();
    let mode = mode_from(opts);
    // --stream: keep listening for the whole duration and print each card as a JSON
    // line the moment it is seen (used by the bridge for live detection).
    let stream = opts.contains_key("stream");

    let found: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));

    // Timer thread: stop at `seconds` max. In non-stream mode also end ~4s after the
    // first card is seen so the device session isn't held longer than needed.
    {
        let stop = stop.clone();
        let found_timer = found.clone();
        std::thread::spawn(move || {
            let start = Instant::now();
            let mut first_at: Option<Instant> = None;
            loop {
                if start.elapsed() >= Duration::from_secs(seconds) {
                    break;
                }
                if !stream && first_at.is_none() {
                    if let Ok(v) = found_timer.lock() {
                        if !v.is_empty() {
                            first_at = Some(Instant::now());
                        }
                    }
                }
                if let Some(t) = first_at {
                    if t.elapsed() >= Duration::from_secs(4) {
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            stop.store(true, Ordering::Relaxed);
        });
    }

    let found_cb = found.clone();
    let res = scan_syslog_for_cards(
        udid.as_deref(),
        mode,
        stop.clone(),
        |hash, name| {
            if let Ok(mut v) = found_cb.lock() {
                if !v.iter().any(|(h, _)| *h == hash) {
                    if stream {
                        println!("{}", json!({ "event": "card", "hash": hash, "name": name }));
                    }
                    v.push((hash, name));
                }
            }
        },
        |line| eprintln!("{line}"),
    );
    stop.store(true, Ordering::Relaxed);
    res.context("Scan failed")?;

    let cards: Vec<_> = found
        .lock()
        .map_err(|_| anyhow!("lock"))?
        .iter()
        .map(|(h, n)| json!({ "hash": h, "name": n }))
        .collect();
    Ok(json!({ "ok": true, "cards": cards }))
}

fn cmd_apply(opts: &HashMap<String, String>) -> Result<serde_json::Value> {
    let image_path = opts.get("image").filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("--image is required"))?;
    let card = opts.get("card").filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("--card <hash> is required"))?;
    let mode = mode_from(opts);
    let udid = resolve_udid(opts)?;

    eprintln!("Preparing image {image_path} ...");
    // Detect the format from the bytes, not the extension (the bridge hands us a temp ".img" file).
    let bytes = std::fs::read(image_path).with_context(|| format!("Could not read image {image_path}"))?;
    let image = image::load_from_memory(&bytes).with_context(|| format!("Could not open image {image_path}"))?;
    let focus_x: f32 = opts.get("focus-x").and_then(|s| s.parse().ok()).unwrap_or(0.5);
    let focus_y: f32 = opts.get("focus-y").and_then(|s| s.parse().ok()).unwrap_or(0.5);
    let skin = PreparedSkin::from_image_with_focus(image, focus_x, focus_y).context("Could not prepare skin")?;

    flash_wallet_skin(
        &udid,
        mode,
        card,
        &skin.png,
        &skin.pdf,
        |step, total, msg| eprintln!("[{step}/{total}] {msg}"),
        |line| eprintln!("{line}"),
    )
    .context("Flashing the skin failed")?;

    Ok(json!({ "ok": true, "message": "Card skin applied. Reopen Wallet on the iPhone to see it.", "udid": udid }))
}

fn cmd_restore(opts: &HashMap<String, String>) -> Result<serde_json::Value> {
    let card = opts.get("card").filter(|s| !s.is_empty()).ok_or_else(|| anyhow!("--card <hash> is required"))?;
    let mode = mode_from(opts);
    let udid = resolve_udid(opts)?;

    restore_wallet_original(
        &udid,
        mode,
        card,
        |step, total, msg| eprintln!("[{step}/{total}] {msg}"),
        |line| eprintln!("{line}"),
    )
    .context("Restore failed")?;

    Ok(json!({ "ok": true, "message": "Original card artwork restored.", "udid": udid }))
}
