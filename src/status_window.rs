// A small "Skini is running" window shown while the bridge runs after a double-click.
// The real UI is the website; this only tells people that Skini works and lets them
// reopen the Studio or quit (closing the window stops the bridge).

use eframe::egui;

const GREEN: egui::Color32 = egui::Color32::from_rgb(0x1f, 0xb4, 0x5a);

// egui lays text out left to right, so a pure Hebrew line is stored reversed to read correctly.
fn he(s: &str) -> String {
    s.chars().rev().collect()
}

struct StatusApp {
    on_open: Box<dyn Fn()>,
    logo: Option<egui::TextureHandle>,
}

// The brand mark (same picture as the exe icon and the site's header).
fn logo_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let img = image::load_from_memory(include_bytes!("logo128.png")).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture("skini-logo", color, egui::TextureOptions::LINEAR))
}

impl eframe::App for StatusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(egui::Color32::from_rgb(0x16, 0x16, 0x18)).inner_margin(22.0))
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(0xf5, 0xf5, 0xf7));
                ui.horizontal(|ui| {
                    if let Some(t) = &self.logo {
                        ui.add(egui::Image::new((t.id(), egui::vec2(32.0, 32.0))));
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Skini").size(22.0).strong());
                });
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(he("פועל ומחובר ✓")).size(18.0).color(GREEN).strong());
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(he("ממשיכים באתר ובוחרים עיצוב.")).size(14.0));
                    ui.label(egui::RichText::new(he("השאירו את החלון פתוח בזמן ההחלפה.")).size(13.0).color(egui::Color32::from_gray(160)));
                    ui.add_space(18.0);
                    let open = egui::Button::new(egui::RichText::new(he("לסטודיו באתר")).size(15.0).color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(0x0a, 0x84, 0xff))
                        .corner_radius(18.0)
                        .min_size(egui::vec2(220.0, 38.0));
                    if ui.add(open).clicked() {
                        (self.on_open)();
                    }
                    ui.add_space(8.0);
                    let close = egui::Button::new(egui::RichText::new(he("סגירה")).size(13.0))
                        .fill(egui::Color32::from_rgb(0x2c, 0x2c, 0x2e))
                        .corner_radius(14.0)
                        .min_size(egui::vec2(220.0, 30.0));
                    if ui.add(close).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}

/// Shows the window and returns when the user closes it (false if it could not open at all).
pub fn show(on_open: impl Fn() + 'static) -> bool {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("logo128.png")).ok();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([340.0, 290.0])
        .with_resizable(false)
        .with_maximize_button(false)
        .with_title("Skini");
    if let Some(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native(
        "Skini",
        options,
        Box::new(move |cc| {
            crate::app::setup_custom_fonts(&cc.egui_ctx);
            Ok(Box::new(StatusApp { on_open: Box::new(on_open), logo: logo_texture(&cc.egui_ctx) }))
        }),
    ).is_ok()
}
