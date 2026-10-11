use eframe::egui::{self, Color32, Stroke, Vec2};
use std::time::{Duration, Instant};

struct App {
    selected: usize,
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
            selected: 1,
            dark,
            popup,
            had_focus: false,
            close_at: auto_close.map(|s| Instant::now() + Duration::from_secs(s)),
            screenshot,
            screenshot_requested: false,
        }
    }

    fn panel(&mut self, ui: &mut egui::Ui) {
        let origin = ui.max_rect().min;
        let scale = (ui.max_rect().width() / 440.0).min(ui.max_rect().height() / 606.0);
        let rect = |x: f32, y: f32, w: f32, h: f32| {
            egui::Rect::from_min_size(origin + Vec2::new(x, y) * scale, Vec2::new(w, h) * scale)
        };
        let fg = if self.dark {
            Color32::from_gray(239)
        } else {
            Color32::from_gray(35)
        };
        let secondary = if self.dark {
            Color32::from_gray(178)
        } else {
            Color32::from_gray(85)
        };
        let muted = if self.dark {
            Color32::from_gray(133)
        } else {
            Color32::from_gray(105)
        };
        let card = if self.dark {
            Color32::from_gray(49)
        } else {
            Color32::from_gray(226)
        };
        let selected = if self.dark {
            Color32::from_gray(62)
        } else {
            Color32::from_gray(207)
        };
        let track = if self.dark {
            Color32::from_gray(55)
        } else {
            Color32::from_gray(203)
        };
        let green = Color32::from_rgb(51, 202, 122);
        let cyan = if self.dark {
            Color32::from_rgb(49, 205, 238)
        } else {
            Color32::from_rgb(0, 113, 148)
        };
        let data = [
            (
                "Cursor",
                "Cursor",
                "pro · Monthly",
                6,
                "of the monthly limit",
                "On pace for ~67%",
                "Monthly",
            ),
            (
                "Codex",
                "ChatGPT Codex",
                "pro · Weekly",
                33,
                "of the 7-day limit",
                "On pace for ~368% (over)",
                "Weekly",
            ),
            (
                "OpenCode",
                "OpenCode Go",
                "Go · Weekly",
                0,
                "of the weekly limit",
                "On pace for ~0%",
                "Weekly",
            ),
        ];
        for (index, provider) in data.iter().enumerate() {
            let x = 4.0 + index as f32 * 147.0;
            let bounds = rect(x, 5.0, 139.0, 92.0);
            let response = ui.interact(bounds, ui.id().with(index), egui::Sense::click());
            if response.clicked() {
                self.selected = index;
            }
            ui.painter().rect_filled(
                bounds,
                9.0 * scale,
                if self.selected == index {
                    selected
                } else {
                    card
                },
            );
            label(
                ui.painter(),
                origin,
                scale,
                x + 9.0,
                21.0,
                provider.0,
                13.0,
                secondary,
            );
            ring(
                ui.painter(),
                origin + Vec2::new(x + 31.0, 61.0) * scale,
                17.0 * scale,
                5.0 * scale,
                provider.3 as f32 / 100.0,
                [0.4, 0.09, 0.0][index],
                track,
                green,
            );
            label(
                ui.painter(),
                origin,
                scale,
                x + 56.0,
                61.0,
                &format!("{}%", provider.3),
                13.0,
                fg,
            );
            if self.selected == index {
                ui.painter()
                    .rect_filled(rect(x + 62.0, 97.0, 14.0, 3.0), 2.0 * scale, secondary);
            }
        }
        let scroll: f32 = ui.input(|input| {
            input
                .raw
                .events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                    _ => None,
                })
                .sum()
        });
        if scroll.abs() > 0.0 {
            self.selected =
                (self.selected as isize + if scroll < 0.0 { 1 } else { -1 }).rem_euclid(3) as usize;
        }
        let p = data[self.selected];
        label(ui.painter(), origin, scale, 23.0, 132.0, p.1, 17.0, fg);
        ui.painter()
            .rect_filled(rect(253.0, 116.0, 100.0, 27.0), 14.0 * scale, track);
        label(
            ui.painter(),
            origin,
            scale,
            263.0,
            130.0,
            p.2,
            10.0,
            secondary,
        );
        label(
            ui.painter(),
            origin,
            scale,
            362.0,
            132.0,
            "Usage >",
            12.0,
            cyan,
        );
        let center = origin + Vec2::new(76.0, 205.0) * scale;
        ring(
            ui.painter(),
            center,
            43.0 * scale,
            5.0 * scale,
            p.3 as f32 / 100.0,
            [0.4, 0.09, 0.0][self.selected],
            track,
            green,
        );
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            format!("{}%", p.3),
            egui::FontId::proportional(17.0 * scale),
            fg,
        );
        label(
            ui.painter(),
            origin,
            scale,
            146.0,
            176.0,
            &format!("{}%", p.3),
            28.0,
            fg,
        );
        label(ui.painter(), origin, scale, 146.0, 202.0, p.4, 13.0, muted);
        label(
            ui.painter(),
            origin,
            scale,
            146.0,
            228.0,
            p.5,
            13.0,
            secondary,
        );
        ui.painter().rect_filled(
            rect(23.0, 332.0, 396.0, 35.0),
            7.0 * scale,
            if self.dark {
                Color32::from_gray(67)
            } else {
                Color32::from_gray(209)
            },
        );
        label(ui.painter(), origin, scale, 39.0, 351.0, "Limits", 13.0, fg);
        ui.painter().rect_filled(
            rect(23.0, 381.0, 396.0, 171.0),
            7.0 * scale,
            if self.dark {
                Color32::from_gray(44)
            } else {
                Color32::from_gray(235)
            },
        );
        label(
            ui.painter(),
            origin,
            scale,
            36.0,
            403.0,
            "Rate limits",
            13.0,
            fg,
        );
        label(
            ui.painter(),
            origin,
            scale,
            36.0,
            428.0,
            p.6,
            13.0,
            secondary,
        );
        label(
            ui.painter(),
            origin,
            scale,
            251.0,
            428.0,
            "resets 18 Oct",
            10.0,
            muted,
        );
        ui.painter().text(
            origin + Vec2::new(395.0, 428.0) * scale,
            egui::Align2::RIGHT_CENTER,
            format!("{}%", p.3),
            egui::FontId::proportional(12.0 * scale),
            fg,
        );
        ui.painter().rect_filled(
            rect(36.0, 439.0, 365.0, 8.0),
            4.0 * scale,
            if self.dark {
                Color32::from_gray(71)
            } else {
                Color32::from_gray(197)
            },
        );
        if p.3 > 0 {
            ui.painter().rect_filled(
                rect(36.0, 439.0, 365.0 * p.3 as f32 / 100.0, 8.0),
                0.0,
                green,
            );
        }
        label(
            ui.painter(),
            origin,
            scale,
            23.0,
            583.0,
            "Resets 18 Oct (7 days)",
            11.0,
            muted,
        );
        ui.painter().text(
            origin + Vec2::new(415.0, 583.0) * scale,
            egui::Align2::RIGHT_CENTER,
            "Updated 3 min ago",
            egui::FontId::proportional(11.0 * scale),
            muted,
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
            .frame(egui::Frame::NONE.fill(if self.dark {
                Color32::from_gray(40)
            } else {
                Color32::from_gray(246)
            }))
            .show(ui, |ui| self.panel(ui));
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
            .with_inner_size([440.0, 606.0])
            .with_resizable(false)
            .with_title("Usage · egui prototype")
            .with_decorations(false),
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

fn label(
    painter: &egui::Painter,
    origin: egui::Pos2,
    scale: f32,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    color: Color32,
) {
    painter.text(
        origin + Vec2::new(x, y) * scale,
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(size * scale),
        color,
    );
}

fn ring(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    width: f32,
    fraction: f32,
    elapsed: f32,
    track: Color32,
    green: Color32,
) {
    painter.circle_stroke(center, radius, Stroke::new(width, track));
    if fraction > 0.0 {
        let points = (0..=48)
            .map(|step| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * fraction * step as f32 / 48.0;
                center + Vec2::new(angle.cos(), angle.sin()) * radius
            })
            .collect::<Vec<_>>();
        painter.add(egui::Shape::line(points, Stroke::new(width, green)));
    }
    let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * elapsed;
    let direction = Vec2::new(angle.cos(), angle.sin());
    painter.line_segment(
        [
            center + direction * (radius - width),
            center + direction * (radius + 1.0),
        ],
        Stroke::new(1.5, Color32::from_gray(225)),
    );
}
