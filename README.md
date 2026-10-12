# Skini Desktop

Free, open-source (MIT) Windows and macOS helper for [Skini](https://www.applepay.skin):
it applies a card design to an iPhone connected by USB cable. It changes card
**artwork only** and never reads or touches payment data.

- **License:** MIT, see `LICENSE` (fork of AirCard, see `NOTICE.md` and `LICENSE.aircard`).
- **Download:** https://www.applepay.skin/download (free).
- **Code signing policy:** Windows releases are built from this repository and signed
  with an open-source code signing certificate. Only binaries built from the code in
  this repository are signed; the signing key is never used for anything else.


The native helper that actually applies a card skin to a USB-connected iPhone.
Fork of AirCard (MIT — see `NOTICE.md`). Changes card **artwork only**; never
touches payment data.

**Double-clicking it (no arguments) runs the built-in bridge** on 127.0.0.1:7879 and
opens the Studio in the browser — this is the whole product: one file, no Node, the
website is the UI. Commands:

```
skini                 # default: run the local bridge + open the Studio (the product)
skini serve [--open]  # just the bridge (what the browser talks to)
skini gui             # the original desktop GUI
skini list            # JSON: connected iPhones
skini scan --seconds 25   # JSON: tap your card in Wallet to reveal its hash
skini apply --image <file> --card <hash>   # apply a skin to that card (scripting)
skini restore --card <hash>                # revert to the original artwork
```

### Bridge environment
| Variable | Default | Meaning |
|---|---|---|
| `SKINI_SITE` | `https://www.applepay.skin/app` | Page opened on launch. |
| `SKINI_ALLOWED_ORIGINS` | localhost + `https://www.applepay.skin` | Web origins allowed to call the bridge. |
| `SKINI_SIMULATE` | — | `1` = pretend to apply (demos without a device). |

The browser (same origin as the Skini server) fetches the licensed skin bytes and
hands them to the bridge as base64 — so the exe needs no network/TLS stack, and the
server still enforces which skins a license may download.

Machine-readable result is a single JSON object on **stdout**; progress/logs go to
**stderr**. Exit code 0 on success, 1 on error (`{"ok":false,"error":"..."}`).

## Build

Needs Rust. Two linker options:

**A — MSVC (recommended for release, needs VS Build Tools "Desktop C++"):**
```
cargo build --release
# -> target/release/skini.exe
```

**B — GNU / MinGW (no admin, no Visual Studio) — used for the current build:**
```
rustup toolchain install stable-x86_64-pc-windows-gnu
# Install a FULL MinGW-w64 (winlibs UCRT) so that %USERPROFILE%\mingw64\bin\as.exe exists.
# (rustup's bundled mingw lacks `as`, which dlltool needs for the windows-* crates.)
.\build.ps1          # adds mingw64\bin to PATH and runs: cargo build --release
# -> target/release/skini.exe
```

## Use with the companion

Point the companion at the built binary and switch it to the real backend:

```
set SKINI_DEVICE_BACKEND=aircard
set SKINI_AIRCARD_PATH=C:\path\to\skini.exe
node companion.js
```

Requirements on the user's PC: Windows 10/11, Apple Mobile Device drivers (ship with
iTunes / Apple Devices), and an iPhone (iOS 18+) connected by cable and trusted.
