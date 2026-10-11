#[path = "../../shared/fixture.rs"]
mod fixture;

use eframe::egui::{self, Align, Color32, Layout, RichText, Stroke, Vec2};
use fixture::{Provider, DAYS, PROVIDERS};
use std::time::{Duration, Instant};

struct App {
    selected: usize,
    tab: usize,
    dark: bool,
    popup: bool,
    had_focus: bool,
    close_at: Option<Instant>,
    screenshot: Option<std::path::PathBuf>,
    screenshot_requested: bool,
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        popup: bool,
        dark: bool,
        auto_close: Option<u64>,
        screenshot: Option<std::path::PathBuf>,
    ) -> Self {
        cc.egui_ctx.set_visuals(if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        });
        Self {
            selected: 0,
            tab: 0,
            dark,
            popup,
            had_focus: false,
            close_at: auto_close.map(|s| Instant::now() + Duration::from_secs(s)),
            screenshot,
            screenshot_requested: false,
        }
    }

    fn panel(&mut self, ui: &mut egui::Ui, p: Provider) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("THIS CYCLE")
                        .size(11.0)
                        .strong()
                        .color(Color32::from_gray(145)),
                );
                ui.add_space(7.0);
                ui.label(RichText::new(p.used).size(31.0).strong());
                ui.label(
                    RichText::new(format!("of {} · resets in 12 days", p.limit))
                        .size(12.0)
                        .color(Color32::from_gray(150)),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(102.0), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                let center = rect.center();
                let radius = 39.0;
                painter.circle_stroke(center, radius, Stroke::new(9.0_f32, Color32::from_gray(55)));
                let sweep = std::f32::consts::TAU * p.percent as f32 / 100.0;
                let points = (0..=48)
                    .map(|i| {
                        let angle = -std::f32::consts::FRAC_PI_2 + sweep * i as f32 / 48.0;
                        center + Vec2::new(angle.cos(), angle.sin()) * radius
                    })
                    .collect::<Vec<_>>();
                painter.add(egui::Shape::line(
                    points,
                    Stroke::new(
                        9.0_f32,
                        Color32::from_rgb(p.color[0], p.color[1], p.color[2]),
                    ),
                ));
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    format!("{}%", p.percent),
                    egui::FontId::proportional(20.0),
                    ui.visuals().text_color(),
                );
            });
        });
        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Usage limit").strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{}%", p.percent)).color(Color32::from_gray(150)));
            });
        });
        ui.add_space(8.0);
        let mut bar = egui::ProgressBar::new(p.percent as f32 / 100.0)
            .desired_height(8.0)
            .fill(Color32::from_rgb(p.color[0], p.color[1], p.color[2]));
        bar = bar.show_percentage();
        ui.add(bar);
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match self.tab {
                    0 => "Recent activity",
                    1 => "Cycle activity",
                    _ => "Usage history",
                })
                .strong(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("Last 12 days")
                        .size(11.0)
                        .color(Color32::from_gray(145)),
                );
            });
        });
        ui.add_space(10.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 132.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        for row in 0..4 {
            let y = rect.top() + row as f32 * 31.0 + 8.0;
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                Stroke::new(1.0_f32, Color32::from_gray(55)),
            );
        }
        let gap = rect.width() / DAYS.len() as f32;
        for (i, value) in DAYS.iter().enumerate() {
            let height = *value as f32 / 100.0 * 92.0;
            let x = rect.left() + i as f32 * gap + gap * 0.22;
            let bar = egui::Rect::from_min_size(
                egui::pos2(x, rect.bottom() - height - 18.0),
                Vec2::new(gap * 0.56, height),
            );
            painter.rect_filled(
                bar,
                3.0,
                Color32::from_rgb(p.color[0], p.color[1], p.color[2]),
            );
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            for label in ["1", "3", "5", "7", "9", "11"] {
                ui.add_sized(
                    Vec2::new(ui.available_width() / 6.0, 14.0),
                    egui::Label::new(
                        RichText::new(label)
                            .size(10.0)
                            .color(Color32::from_gray(130)),
                    ),
                );
            }
        });
        ui.add_space(18.0);
        ui.label(
            RichText::new("All values are sample data")
                .size(11.0)
                .color(Color32::from_gray(130)),
        );
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(path) = &self.screenshot {
            if !self.screenshot_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    path.clone(),
                )));
                self.screenshot_requested = true;
            }
            let capture = ctx.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot {
                        user_data, image, ..
                    } => user_data
                        .data
                        .as_ref()
                        .and_then(|data| data.downcast_ref::<std::path::PathBuf>())
                        .map(|p| (p.clone(), image.clone())),
                    _ => None,
                })
            });
            if let Some((path, image)) = capture {
                let result = save_png(
                    &path,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    &image
                        .pixels
                        .iter()
                        .flat_map(|c| c.to_array())
                        .collect::<Vec<_>>(),
                );
                match result {
                    Ok(()) => eprintln!("screenshot saved: {}", path.display()),
                    Err(err) => eprintln!("screenshot failed: {err}"),
                }
                if self.close_at.is_none() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.close_at.is_some_and(|t| Instant::now() >= t) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.close_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(ui.style()).inner_margin(24))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Usage").size(20.0).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(if self.dark { "Light" } else { "Dark" })
                            .on_hover_text("Toggle theme")
                            .clicked()
                        {
                            self.dark = !self.dark;
                            ctx.set_visuals(if self.dark {
                                egui::Visuals::dark()
                            } else {
                                egui::Visuals::light()
                            });
                        }
                        ui.label(
                            RichText::new("All systems normal")
                                .size(11.0)
                                .color(Color32::from_rgb(103, 213, 163)),
                        );
                    });
                });
                ui.add_space(18.0);
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(PROVIDERS[self.selected].name)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for (i, p) in PROVIDERS.iter().enumerate() {
                            ui.selectable_value(&mut self.selected, i, p.name);
                        }
                    });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    for (i, tab) in ["Limits", "This cycle", "History"].iter().enumerate() {
                        if ui.selectable_label(self.tab == i, *tab).clicked() {
                            self.tab = i;
                        }
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .show(ui, |ui| self.panel(ui, PROVIDERS[self.selected]));
            });
        if ctx.input(|i| i.viewport().focused == Some(true)) {
            self.had_focus = true;
        }
        if self.popup && self.had_focus && ctx.input(|i| i.viewport().focused == Some(false)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn main() -> eframe::Result<()> {
    let mut popup = false;
    let mut auto_close = None;
    let mut screenshot = None;
    let mut dark = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--popup" => popup = true,
            "--light" => dark = false,
            "--auto-close" => auto_close = args.next().and_then(|s| s.parse().ok()),
            "--screenshot" => screenshot = args.next().map(Into::into),
            "--x11" => {
                std::env::remove_var("WAYLAND_DISPLAY");
                std::env::remove_var("WAYLAND_SOCKET");
            }
            "--wayland" => {
                std::env::remove_var("DISPLAY");
            }
            _ => {}
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([440.0, 600.0])
            .with_resizable(false)
            .with_title("Usage · egui prototype")
            .with_decorations(!popup),
        ..Default::default()
    };
    eframe::run_native(
        "Usage · egui prototype",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, popup, dark, auto_close, screenshot)))),
    )
}

fn save_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}
