// Skini Desktop — fork of AirCard (MIT, Mak5er/AirCard & Lumid-Off/AirCard-Windows).
// With no arguments it launches the GUI; with a command it runs headless so the
// Skini web companion can drive it (see cli.rs).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod afc;
mod airlift;
mod airtraffic;
mod app;
mod apple;
mod cli;
mod device;
mod flasher;
mod image_skin;
mod i18n;
mod passthm;
mod platform;
mod scanner;
mod serve;
mod wallet_backup;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> eframe::Result<()> {
    // Finder on older macOS adds a -psn_... argument when it opens an app bundle.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();

    // Default (double-click, no args): run the local bridge the website talks to,
    // and open the Studio in the browser. The whole UI lives in the browser.
    // Also the entry point when the website launches us through skini://open
    // (the URL arrives as the first argument) or at startup (`serve --quiet`).
    let from_url = args.first().map(|s| s.starts_with("skini://")).unwrap_or(false);
    if args.is_empty() || from_url || args.first().map(|s| s.as_str()) == Some("serve") {
        let quiet = from_url || args.iter().any(|a| a == "--quiet");
        let open = args.is_empty() || args.iter().any(|a| a == "--open");
        std::process::exit(serve::run(open, quiet));
    }

    // Headless commands for power users / scripting.
    if cli::is_cli(&args) {
        std::process::exit(cli::run(&args));
    }

    // `skini gui` opens the original desktop interface.
    let title = format!("Skini v{VERSION}");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 640.0])
            .with_min_inner_size([880.0, 560.0])
            .with_title(&title),
        ..Default::default()
    };

    eframe::run_native(
        &title,
        options,
        Box::new(|cc| Ok(Box::new(app::AirCardApp::new(cc)))),
    )
}
