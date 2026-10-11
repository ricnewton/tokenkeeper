#[path = "../../shared/fixture.rs"]
mod fixture;

use fixture::{Provider, DAYS, PROVIDERS};
use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, text, Column, Row,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Size, Theme};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
enum Message {
    Provider(Provider),
    Tab(usize),
    ToggleTheme,
    Tick,
    Screenshot(iced::window::Screenshot),
    Latest(Option<iced::window::Id>),
    Focused,
    LostFocus,
    Exit,
}

struct App {
    selected: Provider,
    tab: usize,
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
            selected: PROVIDERS[0],
            tab: 0,
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
            Message::Tab(tab) => self.tab = tab,
            Message::ToggleTheme => self.dark = !self.dark,
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
        let title = row![
            text("Usage").size(21).font(iced::Font::DEFAULT),
            iced::widget::Space::new().width(Length::Fill),
            text("All systems normal")
                .size(11)
                .color(Color::from_rgb(0.40, 0.84, 0.64)),
            button(text(if self.dark { "Light" } else { "Dark" }))
                .on_press(Message::ToggleTheme)
                .padding([5, 9])
        ]
        .align_y(Alignment::Center)
        .spacing(9);
        let choices = PROVIDERS.to_vec();
        let provider = pick_list(choices, Some(self.selected), Message::Provider)
            .placeholder("Choose provider")
            .width(Length::Fill);
        let mut tabs = Row::new().spacing(8);
        for (i, label) in ["Limits", "This cycle", "History"].iter().enumerate() {
            let active = self.tab == i;
            tabs = tabs.push(
                button(
                    text(if active {
                        format!("● {label}")
                    } else {
                        (*label).to_string()
                    })
                    .size(13),
                )
                .on_press(Message::Tab(i))
                .padding([8, 12]),
            );
        }
        let p = self.selected;
        let rgb = Color::from_rgb8(p.color[0], p.color[1], p.color[2]);
        let summary = row![
            column![
                text("THIS CYCLE")
                    .size(11)
                    .color(Color::from_rgb(0.56, 0.57, 0.61)),
                text(p.used).size(31),
                text(format!("of {} · resets in 12 days", p.limit))
                    .size(12)
                    .color(Color::from_rgb(0.56, 0.57, 0.61))
            ]
            .spacing(7),
            iced::widget::Space::new().width(Length::Fill),
            iced::widget::canvas(Ring {
                percent: p.percent,
                color: rgb
            })
            .width(92)
            .height(92)
        ]
        .align_y(Alignment::Center)
        .spacing(12)
        .padding([0, 0]);
        let usage = column![
            row![
                text("Usage limit").size(14),
                iced::widget::Space::new().width(Length::Fill),
                text(format!("{}%", p.percent))
                    .size(12)
                    .color(Color::from_rgb(0.56, 0.57, 0.61))
            ]
            .align_y(Alignment::Center),
            progress_bar(0.0..=100.0, p.percent as f32).girth(8)
        ]
        .spacing(10);
        let mut chart = Row::new()
            .spacing(6)
            .align_y(Alignment::End)
            .height(Length::Fixed(100.0));
        for value in DAYS {
            let bar = container(
                iced::widget::Space::new()
                    .height(Length::Fixed(value as f32 * 0.9))
                    .width(Length::Fill),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(rgb)),
                border: Border::default().rounded(3),
                ..container::Style::default()
            });
            chart = chart.push(
                container(bar)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom),
            );
        }
        let activity = column![
            row![
                text(match self.tab {
                    0 => "Recent activity",
                    1 => "Cycle activity",
                    _ => "Usage history",
                })
                .size(14),
                iced::widget::Space::new().width(Length::Fill),
                text("Last 12 days")
                    .size(11)
                    .color(Color::from_rgb(0.56, 0.57, 0.61))
            ]
            .align_y(Alignment::Center),
            chart,
            row![
                text("1          3          5          7          9         11")
                    .size(10)
                    .color(Color::from_rgb(0.56, 0.57, 0.61))
            ]
        ]
        .spacing(12);
        let body: Column<'_, Message> = column![
            title,
            provider,
            tabs,
            summary,
            usage,
            activity,
            text("All values are sample data")
                .size(11)
                .color(Color::from_rgb(0.56, 0.57, 0.61))
        ]
        .spacing(12)
        .width(376);
        container(scrollable(body).height(Length::Fill).width(Length::Fill))
            .width(Length::Fill)
            .padding(20)
            .style(|theme| container::Style {
                background: Some(Background::Color(theme.palette().background)),
                ..container::Style::default()
            })
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
        size: Size::new(440.0, 600.0),
        ..iced::window::Settings::default()
    })
    .decorations(!_popup)
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

struct Ring {
    percent: u8,
    color: Color,
}
impl iced::widget::canvas::Program<Message> for Ring {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke, Text};
        let mut frame = Frame::new(renderer, bounds.size());
        let center = frame.center();
        frame.stroke(
            &Path::circle(center, 39.0),
            Stroke::default()
                .with_width(9.0)
                .with_color(Color::from_rgb(0.3, 0.3, 0.3)),
        );
        let arc = Path::new(|builder| {
            for i in 0..=48 {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * self.percent as f32 / 100.0 * i as f32 / 48.0;
                let point =
                    iced::Point::new(center.x + angle.cos() * 39.0, center.y + angle.sin() * 39.0);
                if i == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &arc,
            Stroke::default().with_width(9.0).with_color(self.color),
        );
        frame.fill_text(Text {
            content: format!("{}%", self.percent),
            position: center,
            color: theme.palette().text,
            size: iced::Pixels(20.0),
            align_x: iced::alignment::Horizontal::Center.into(),
            align_y: iced::alignment::Vertical::Center,
            ..Text::default()
        });
        vec![frame.into_geometry()]
    }
}
