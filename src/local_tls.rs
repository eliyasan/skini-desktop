// macOS only. Safari will not let an https website call http://127.0.0.1 (it blocks it as mixed
// content before sending anything), so on the Mac the bridge also listens on https://127.0.0.1:7880.
// The certificate is made on this Mac by the system's own openssl, is valid only for
// 127.0.0.1 / localhost (CA:FALSE, so it cannot vouch for anything else), and the user is asked
// once, by macOS itself, to trust it.
use std::path::PathBuf;
use std::process::Command;

pub const HTTPS_ADDR: &str = "127.0.0.1:7880";

const OPENSSL_CONFIG: &str = "[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Skini Local (127.0.0.1)
O = Skini
[ext]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature,keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = IP:127.0.0.1,DNS:localhost
";

fn dir() -> PathBuf {
    crate::platform::app_data_dir().join("Skini").join("tls")
}

fn cert_path() -> PathBuf {
    dir().join("cert.pem")
}

/// The certificate and private key (PEM), created on first use.
pub fn load_or_create() -> Option<(Vec<u8>, Vec<u8>)> {
    let d = dir();
    let (cert, key) = (cert_path(), d.join("key.pem"));
    if !(cert.is_file() && key.is_file()) {
        std::fs::create_dir_all(&d).ok()?;
        let cfg = d.join("openssl.cnf");
        std::fs::write(&cfg, OPENSSL_CONFIG).ok()?;
        // Apple refuses TLS certificates valid for more than 825 days.
        let ok = Command::new("/usr/bin/openssl")
            .args(["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-sha256", "-days", "800", "-keyout"])
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .arg("-config")
            .arg(&cfg)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            let _ = std::fs::remove_file(&cert);
            let _ = std::fs::remove_file(&key);
            return None;
        }
        // The key never leaves this Mac; keep it readable by this user only.
        let _ = Command::new("/bin/chmod").arg("600").arg(&key).status();
    }
    Some((std::fs::read(&cert).ok()?, std::fs::read(&key).ok()?))
}

pub fn is_trusted() -> bool {
    Command::new("/usr/bin/security")
        .args(["verify-cert", "-p", "ssl", "-c"])
        .arg(cert_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Asks macOS to trust the certificate for SSL. macOS shows its own password / Touch ID prompt.
/// Blocks until the user answers, so call it off the main path.
pub fn ensure_trusted() {
    if std::env::var_os("SKINI_SKIP_TRUST").is_some() || is_trusted() {
        return;
    }
    let keychain = crate::platform::home_dir().join("Library/Keychains/login.keychain-db");
    let ok = Command::new("/usr/bin/security")
        .args(["add-trusted-cert", "-r", "trustRoot", "-p", "ssl", "-k"])
        .arg(&keychain)
        .arg(cert_path())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    eprintln!("Local certificate trust: {}", if ok && is_trusted() { "ok" } else { "not granted" });
}
