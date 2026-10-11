use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Length, Point, Rectangle, Size, Theme};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
enum Message {
    Provider(usize),
    Cycle(i32),
    Tick,
    Screenshot(iced::window::Screenshot),
    Latest(Option<iced::window::Id>),
    Focused,
    LostFocus,
    Exit,
}

struct App {
    selected: usize,
    dark: bool,
    close_at: Option<Instant>,
    screenshot: Option<std::path::PathBuf>,
    screenshot_requested: bool,
    window_id: Option<iced::window::Id>,
    had_focus: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            selected: 1,
            dark: true,
            close_at: None,
            screenshot: None,
            screenshot_requested: false,
            window_id: None,
            had_focus: false,
        }
    }
}

impl App {
    fn update(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::Provider(p) => self.selected = p,
            Message::Cycle(step) => {
                self.selected = (self.selected as i32 + step).rem_euclid(3) as usize
            }
            Message::Tick => {
                if self.close_at.is_some_and(|t| Instant::now() >= t) {
                    return iced::exit();
                }
                if self.screenshot.is_some() && !self.screenshot_requested {
                    self.screenshot_requested = true;
                    return iced::window::latest().map(Message::Latest);
                }
            }
            Message::Latest(Some(id)) => {
                self.window_id = Some(id);
                return iced::window::screenshot(id).map(Message::Screenshot);
            }
            Message::Latest(None) => self.screenshot_requested = false,
            Message::Focused => self.had_focus = true,
            Message::LostFocus => {
                if self.had_focus {
                    return iced::exit();
                }
            }
            Message::Exit => return iced::exit(),
            Message::Screenshot(image) => {
                if let Some(path) = &self.screenshot {
                    match save_png(path, image.size.width, image.size.height, image.as_ref()) {
                        Ok(()) => eprintln!("screenshot saved: {}", path.display()),
                        Err(err) => eprintln!("screenshot failed: {err}"),
                    }
                    if self.close_at.is_none() {
                        if let Some(id) = self.window_id {
                            return iced::window::close(id);
                        }
                    }
                }
            }
        }
        iced::Task::none()
    }

    fn view(&self) -> Element<'_, Message, Theme> {
        iced::widget::canvas(Panel {
            selected: self.selected,
            light: !self.dark,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}

pub fn main() -> iced::Result {
    let mut _popup = false;
    let mut auto_close = None;
    let mut screenshot = None;
    let mut dark = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--popup" => _popup = true,
            "--light" => dark = false,
            "--auto-close" => auto_close = args.next().and_then(|s| s.parse::<u64>().ok()),
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
    let screenshot_for_sub = screenshot.clone();
    let app = iced::application(
        move || {
            let mut state = App::default();
            state.dark = dark;
            state.close_at = auto_close.map(|s| Instant::now() + Duration::from_secs(s));
            state.screenshot = screenshot.clone();
            state
        },
        App::update,
        App::view,
    )
    .title("Usage · Iced prototype")
    .theme(|state: &App| {
        if state.dark {
            Theme::Dark
        } else {
            Theme::Light
        }
    })
    .window(iced::window::Settings {
        size: Size::new(440.0, 606.0),
        ..iced::window::Settings::default()
    })
    .decorations(false)
    .resizable(false)
    .subscription(move |_state: &App| {
        let keyboard = iced::event::listen_with(|event, _status, _id| match event {
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            }) => Some(Message::Exit),
            _ => None,
        });
        let events = if _popup {
            iced::window::events().map(|(_, event)| match event {
                iced::window::Event::Focused => Message::Focused,
                iced::window::Event::Unfocused => Message::LostFocus,
                _ => Message::Tick,
            })
        } else {
            iced::Subscription::none()
        };
        let timer = if auto_close.is_some() || screenshot_for_sub.is_some() {
            iced::time::every(Duration::from_millis(250)).map(|_| Message::Tick)
        } else {
            iced::Subscription::none()
        };
        iced::Subscription::batch([keyboard, events, timer])
    });
    app.run()
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

const ACCOUNTS: [(&str, &str, u8, &str, &str, &str, &str, f32); 3] = [
    (
        "Cursor",
        "Cursor",
        6,
        "pro · Monthly",
        "of the monthly limit",
        "On pace for ~67%",
        "Monthly",
        0.4,
    ),
    (
        "Codex",
        "ChatGPT Codex",
        33,
        "pro · Weekly",
        "of the 7-day limit",
        "On pace for ~368% (over)",
        "Weekly",
        0.09,
    ),
    (
        "OpenCode",
        "OpenCode Go",
        0,
        "Go · Weekly",
        "of the weekly limit",
        "On pace for ~0%",
        "Weekly",
        0.0,
    ),
];

struct Panel {
    selected: usize,
    light: bool,
}
impl canvas::Program<Message> for Panel {
    type State = ();
    fn update(
        &self,
        _: &mut (),
        event: &iced::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let point = cursor.position_in(bounds)?;
        match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if point.y >= 5.0 && point.y <= 97.0 =>
            {
                let index = (point.x / (440.0 / 3.0)).floor() as usize;
                if index < 3
                    && point.x >= index as f32 * (440.0 / 3.0) + 4.0
                    && point.x <= (index + 1) as f32 * (440.0 / 3.0) - 4.0
                {
                    return Some(canvas::Action::publish(Message::Provider(index)).and_capture());
                }
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                let y = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. }
                    | iced::mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if y != 0.0 {
                    return Some(
                        canvas::Action::publish(Message::Cycle(if y > 0.0 { -1 } else { 1 }))
                            .and_capture(),
                    );
                }
            }
            _ => {}
        }
        None
    }
    fn mouse_interaction(
        &self,
        _: &(),
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if cursor
            .position_in(bounds)
            .is_some_and(|p| p.y >= 5.0 && p.y <= 97.0)
        {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let fg = self.gray(1.0);
        let muted = self.gray(0.62);
        f.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            self.rgb(if self.light { 0xf9f9f9 } else { 0x2b2b2b }),
        );
        for (i, account) in ACCOUNTS.iter().enumerate() {
            let x = i as f32 * 440.0 / 3.0;
            self.rounded(
                &mut f,
                x + 4.0,
                5.0,
                440.0 / 3.0 - 8.0,
                92.0,
                9.0,
                if i == self.selected { 0.25 } else { 0.20 },
            );
            self.text(
                &mut f,
                x + 12.0,
                26.0,
                account.0,
                13.0,
                self.gray(if i == self.selected { 0.98 } else { 0.75 }),
            );
            self.ring(&mut f, x + 35.0, 61.0, 17.0, account.2, account.7);
            self.text(
                &mut f,
                x + 59.0,
                65.0,
                &format!("{}%", account.2),
                14.0,
                self.gray(0.95),
            );
            if i == self.selected {
                self.rounded(&mut f, x + 440.0 / 6.0 - 7.0, 97.0, 14.0, 3.0, 2.0, 0.70);
            }
        }
        let p = ACCOUNTS[self.selected];
        self.text(&mut f, 22.0, 137.0, p.1, 17.0, fg);
        self.rounded(&mut f, 252.0, 116.0, 100.0, 27.0, 13.0, 0.23);
        self.text(&mut f, 261.0, 134.0, p.3, 10.0, self.gray(0.75));
        self.text(
            &mut f,
            361.0,
            137.0,
            "Usage >",
            12.0,
            self.rgb(if self.light { 0x00779b } else { 0x58c7eb }),
        );
        self.ring(&mut f, 75.0, 205.0, 43.0, p.2, p.7);
        self.center_text(&mut f, 75.0, 212.0, &format!("{}%", p.2), 17.0, fg);
        self.text(&mut f, 145.0, 184.0, &format!("{}%", p.2), 28.0, fg);
        self.text(&mut f, 145.0, 207.0, p.4, 13.0, muted);
        self.text(&mut f, 145.0, 232.0, p.5, 13.0, self.gray(0.80));
        self.rounded(&mut f, 20.0, 330.0, 400.0, 38.0, 8.0, 0.18);
        self.rounded(&mut f, 22.0, 332.0, 396.0, 34.0, 7.0, 0.27);
        self.text(&mut f, 38.0, 355.0, "Limits", 13.0, fg);
        self.rounded(&mut f, 20.0, 380.0, 400.0, 172.0, 9.0, 0.18);
        self.text(&mut f, 35.0, 408.0, "Rate limits", 13.0, self.gray(0.95));
        self.text(&mut f, 35.0, 432.0, p.6, 12.0, self.gray(0.80));
        self.text(&mut f, 250.0, 432.0, "resets 18 Oct", 10.0, self.gray(0.58));
        self.text(
            &mut f,
            370.0,
            432.0,
            &format!("{}%", p.2),
            12.0,
            self.gray(0.90),
        );
        self.rounded(&mut f, 35.0, 439.0, 365.0, 8.0, 4.0, 0.30);
        if p.2 > 0 {
            f.fill_rectangle(
                Point::new(35.0, 439.0),
                Size::new(365.0 * p.2 as f32 / 100.0, 8.0),
                self.rgb(0x35c878),
            );
        }
        self.text(
            &mut f,
            22.0,
            586.0,
            "Resets 18 Oct (7 days)",
            11.0,
            self.gray(0.56),
        );
        self.text(
            &mut f,
            315.0,
            586.0,
            "Updated 3 min ago",
            11.0,
            self.gray(0.56),
        );
        vec![f.into_geometry()]
    }
}
impl Panel {
    fn rgb(&self, value: u32) -> Color {
        Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }
    fn gray(&self, value: f32) -> Color {
        let v = if self.light { 1.0 - value } else { value };
        Color::from_rgb(v, v, v)
    }
    fn rounded(&self, frame: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32, shade: f32) {
        let path = Path::rounded_rectangle(
            Point::new(x, y),
            Size::new(w, h),
            iced::border::Radius::from(r),
        );
        let color = if self.light {
            let v = 1.0 - shade * 0.25;
            Color::from_rgb(v, v, v)
        } else {
            self.gray(shade)
        };
        frame.fill(&path, color);
    }
    fn text(&self, frame: &mut Frame, x: f32, y: f32, value: &str, size: f32, color: Color) {
        frame.fill_text(Text {
            content: value.into(),
            position: Point::new(x, y - size),
            size: iced::Pixels(size),
            color,
            line_height: iced::widget::text::LineHeight::Absolute(iced::Pixels(size + 2.0)),
            ..Text::default()
        });
    }
    fn center_text(&self, frame: &mut Frame, x: f32, y: f32, value: &str, size: f32, color: Color) {
        frame.fill_text(Text {
            content: value.into(),
            position: Point::new(x, y - size),
            size: iced::Pixels(size),
            color,
            align_x: iced::alignment::Horizontal::Center.into(),
            line_height: iced::widget::text::LineHeight::Absolute(iced::Pixels(size + 2.0)),
            ..Text::default()
        });
    }
    fn ring(&self, frame: &mut Frame, x: f32, y: f32, r: f32, percent: u8, elapsed: f32) {
        frame.stroke(
            &Path::circle(Point::new(x, y), r),
            Stroke::default()
                .with_width(5.0)
                .with_color(self.gray(0.22)),
        );
        if percent > 0 {
            let arc = Path::new(|builder| {
                builder.arc(canvas::path::Arc {
                    center: Point::new(x, y),
                    radius: r,
                    start_angle: iced::Radians(-std::f32::consts::FRAC_PI_2),
                    end_angle: iced::Radians(
                        -std::f32::consts::FRAC_PI_2
                            + std::f32::consts::TAU * percent as f32 / 100.0,
                    ),
                });
            });
            frame.stroke(
                &arc,
                Stroke::default()
                    .with_width(5.0)
                    .with_color(self.rgb(0x35c878)),
            );
        }
        let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * elapsed;
        frame.stroke(
            &Path::line(
                Point::new(x + (r - 4.0) * angle.cos(), y + (r - 4.0) * angle.sin()),
                Point::new(x + (r + 4.0) * angle.cos(), y + (r + 4.0) * angle.sin()),
            ),
            Stroke::default().with_width(2.0).with_color(self.gray(0.9)),
        );
    }
}
