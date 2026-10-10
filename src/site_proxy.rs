// macOS only. Safari does not let a public website talk to 127.0.0.1 at all (it blocks the request
// before connecting, http or https), but it does open http://127.0.0.1 pages normally. So on the Mac
// the Studio is opened from the bridge itself, http://127.0.0.1:7879/app: everything that is not a
// /skini/ call is fetched from the real site and passed through, which puts the page, its /api calls
// and the bridge on one origin. Other pages (home, checkout...) are sent back to the real site.
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

use tiny_http::{Header, Request, Response};

pub const STUDIO_URL: &str = "http://127.0.0.1:7879/app";

fn site_origin() -> String {
    std::env::var("SKINI_SITE_ORIGIN").unwrap_or_else(|_| "https://www.applepay.skin".to_string())
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .redirects(0)
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()
    })
}

fn is_studio_page(path: &str) -> bool {
    matches!(path, "/app" | "/en/app" | "/en2/app" | "/he/app")
}

fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

// Request headers worth passing on. Never Host/Origin/Referer: upstream sees a normal visit.
const FORWARD: &[&str] = &["Accept", "Accept-Language", "Content-Type", "Cookie", "User-Agent", "Range", "If-None-Match", "If-Modified-Since"];
// Response headers worth passing back. Length/encoding are set by tiny_http itself.
const PASS_BACK: &[&str] = &[
    "Content-Type", "Cache-Control", "Content-Disposition", "Content-Language", "ETag", "Last-Modified",
    "Vary", "Content-Range", "Accept-Ranges", "Content-Security-Policy", "X-Content-Type-Options",
    "Referrer-Policy", "Permissions-Policy", "X-Robots-Tag",
];

pub fn handle(mut req: Request) {
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("").to_string();
    let method = req.method().as_str().to_string();
    let origin = site_origin();

    let wants_html = req
        .headers()
        .iter()
        .any(|h| h.field.equiv("Accept") && h.value.as_str().contains("text/html"));
    if method == "GET" && wants_html && !is_studio_page(&path) {
        // Not the Studio: show the real site.
        let mut resp = Response::empty(302);
        if let Some(h) = header("Location", &format!("{origin}{url}")) {
            resp.add_header(h);
        }
        let _ = req.respond(resp);
        return;
    }

    let mut body = Vec::new();
    if method != "GET" && method != "HEAD" {
        let _ = req.as_reader().read_to_end(&mut body);
    }
    let mut up = agent().request(&method, &format!("{origin}{url}")).set("Accept-Encoding", "identity");
    for h in req.headers() {
        if FORWARD.iter().any(|f| h.field.equiv(*f)) {
            up = up.set(h.field.as_str().as_str(), h.value.as_str());
        }
    }
    let result = if body.is_empty() { up.call() } else { up.send_bytes(&body) };
    let resp = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => {
            let msg = format!("Skini could not reach {origin}: {e}");
            let _ = req.respond(Response::from_string(msg).with_status_code(502));
            return;
        }
    };

    let status = resp.status();
    let mut headers: Vec<Header> = Vec::new();
    for name in resp.headers_names() {
        let lname = name.to_ascii_lowercase();
        for value in resp.all(&name) {
            let value = if lname == "location" {
                value.strip_prefix(origin.as_str()).unwrap_or(value).to_string()
            } else if lname == "set-cookie" {
                // Host-only cookie on 127.0.0.1 (a Domain= for the real site would be refused).
                value.split(';').map(str::trim).filter(|p| !p.to_ascii_lowercase().starts_with("domain=")).collect::<Vec<_>>().join("; ")
            } else if PASS_BACK.iter().any(|p| p.eq_ignore_ascii_case(&name)) {
                value.to_string()
            } else {
                continue;
            };
            if let Some(h) = header(&name, &value) {
                headers.push(h);
            }
        }
    }
    let mut data = Vec::new();
    let _ = resp.into_reader().take(64 * 1024 * 1024).read_to_end(&mut data);
    let mut out = Response::from_data(data).with_status_code(status);
    for h in headers {
        out.add_header(h);
    }
    let _ = req.respond(out);
}
