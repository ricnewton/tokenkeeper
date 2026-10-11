#[path = "../../shared/fixture.rs"]
mod fixture;

use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, Bounds, Context, KeyBinding, Render,
    ScrollDelta, Window, WindowBounds, WindowDecorations, WindowKind, WindowOptions,
};
use std::{env, time::Duration};

actions!(usage_panel, [ClosePanel]);

const ACCOUNTS: [(usize, u8); 3] = [(0, 6), (3, 33), (4, 0)];
const GREEN: u32 = 0x31c77b;
const TRACK_DARK: u32 = 0x383838;
const TRACK_LIGHT: u32 = 0xd5d7dc;

struct UsagePanel {
    provider: usize,
    tab: usize,
    dark: bool,
    focus: gpui::FocusHandle,
}

impl UsagePanel {
    fn background(&self) -> u32 {
        if self.dark {
            0x2b2b2b
        } else {
            0xf5f5f5
        }
    }
    fn card(&self, selected: bool) -> u32 {
        if selected {
            if self.dark {
                0x404040
            } else {
                0xffffff
            }
        } else if self.dark {
            0x333333
        } else {
            0xe8e8e8
        }
    }
    fn text(&self) -> u32 {
        if self.dark {
            0xf3f3f3
        } else {
            0x242424
        }
    }
    fn muted(&self) -> u32 {
        if self.dark {
            0x969696
        } else {
            0x737373
        }
    }
}

fn percent(provider: usize) -> u8 {
    ACCOUNTS
        .iter()
        .find(|(id, _)| *id == provider)
        .map_or(0, |(_, value)| *value)
}

fn account_order(provider: usize) -> usize {
    ACCOUNTS
        .iter()
        .position(|(id, _)| *id == provider)
        .unwrap_or(1)
}

fn ring(
    percent: u8,
    elapsed: f32,
    diameter: f32,
    stroke: f32,
    label: bool,
    text_color: u32,
    track: u32,
) -> impl IntoElement {
    let radius = diameter / 2.0 - stroke / 2.0;
    let arc = f32::from(percent.min(100)) / 100.0;
    let canvas = gpui::canvas(
        move |_, _, _| (),
        move |bounds, _, window, _| {
            for (fraction, color) in [(1.0_f32, track), (arc, GREEN)] {
                if fraction <= 0.0 {
                    continue;
                }
                let mut path = gpui::PathBuilder::stroke(px(stroke));
                let center = bounds.center();
                let segments = (64.0 * fraction).ceil() as usize;
                for i in 0..=segments {
                    let angle = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::TAU * fraction * i as f32 / segments.max(1) as f32;
                    let point = gpui::point(
                        center.x + px(angle.cos() * radius),
                        center.y + px(angle.sin() * radius),
                    );
                    if i == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(color));
                }
            }
            {
                let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * elapsed;
                let center = bounds.center();
                let inner = radius - 4.0;
                let outer = radius + 4.0;
                let mut marker = gpui::PathBuilder::stroke(px(1.5));
                marker.move_to(gpui::point(
                    center.x + px(angle.cos() * inner),
                    center.y + px(angle.sin() * inner),
                ));
                marker.line_to(gpui::point(
                    center.x + px(angle.cos() * outer),
                    center.y + px(angle.sin() * outer),
                ));
                if let Ok(marker) = marker.build() {
                    window.paint_path(marker, rgb(0xffffff));
                }
            }
        },
    )
    .absolute()
    .size_full();

    let mut result = div()
        .size(px(diameter))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .child(canvas);
    if label {
        result = result.child(
            div()
                .text_size(px(17.))
                .font_weight(gpui::FontWeight::NORMAL)
                .text_color(rgb(text_color))
                .child(format!("{percent}%")),
        );
    }
    result
}

impl Render for UsagePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.background();
        let foreground = self.text();
        let muted = self.muted();
        let track = if self.dark { TRACK_DARK } else { TRACK_LIGHT };
        let active = self.provider;
        let amount = percent(active);
        let header = match active {
            0 => "Cursor",
            3 => "ChatGPT Codex",
            4 => "OpenCode Go",
            _ => "Usage",
        };
        let plan = match active {
            3 => "pro · Weekly",
            0 => "pro · Monthly",
            _ => "Go · Weekly",
        };
        let elapsed = match active {
            0 => 0.4,
            3 => 0.09,
            _ => 0.0,
        };
        let limit = match active {
            3 => "of the 7-day limit",
            0 => "of the monthly limit",
            _ => "of the weekly limit",
        };
        let pace = if amount == 33 {
            "On pace for ~368% (over)"
        } else if amount == 0 {
            "On pace for ~0%"
        } else {
            "On pace for ~67%"
        };

        let mut cards = div()
            .absolute()
            .top(px(5.))
            .left(px(5.))
            .w(px(430.))
            .h(px(92.))
            .flex()
            .gap(px(8.));
        for (index, (provider_id, value)) in ACCOUNTS.iter().copied().enumerate() {
            let selected = provider_id == active;
            let name = fixture::PROVIDERS[provider_id].name;
            cards = cards.child(
                div()
                    .id(("account-card", index))
                    .flex_1()
                    .h_full()
                    .rounded_lg()
                    .p(px(8.))
                    .bg(rgb(self.card(selected)))
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(if selected { foreground } else { muted }))
                            .child(name),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(ring(
                                value,
                                match provider_id {
                                    0 => 0.4,
                                    3 => 0.09,
                                    _ => 0.0,
                                },
                                40.,
                                5.,
                                false,
                                foreground,
                                track,
                            ))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(foreground))
                                    .child(format!("{value}%")),
                            ),
                    ),
            );
        }

        let selector_left = account_order(active) as f32 * 440.0 / 3.0 + 440.0 / 6.0 - 7.0;
        let selector_mark = div()
            .absolute()
            .top(px(97.))
            .left(px(selector_left))
            .w(px(14.))
            .h(px(3.))
            .rounded_full()
            .bg(rgb(if self.dark { 0xbcbcbc } else { 0x777777 }));

        let header_row = div()
            .absolute()
            .top(px(115.))
            .left(px(22.))
            .w(px(396.))
            .h(px(28.))
            .child(
                div()
                    .absolute()
                    .top(px(3.))
                    .text_size(px(17.))
                    .text_color(rgb(foreground))
                    .child(header),
            )
            .child(
                div()
                    .absolute()
                    .left(px(230.))
                    .top(px(1.))
                    .w(px(100.))
                    .h(px(27.))
                    .rounded_full()
                    .bg(rgb(self.card(false)))
                    .flex()
                    .items_center()
                    .px(px(9.))
                    .text_size(px(10.))
                    .text_color(rgb(muted))
                    .child(plan),
            )
            .child(
                div()
                    .absolute()
                    .left(px(339.))
                    .top(px(5.))
                    .text_size(px(12.))
                    .text_color(rgb(0x62c8e8))
                    .child("Usage >"),
            );

        let summary = div()
            .absolute()
            .top(px(158.))
            .left(px(30.))
            .w(px(390.))
            .h(px(96.))
            .child(
                div()
                    .absolute()
                    .top(px(2.))
                    .child(ring(amount, elapsed, 90., 5., true, foreground, track)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(115.))
                    .top(px(-2.))
                    .text_size(px(28.))
                    .text_color(rgb(foreground))
                    .child(format!("{amount}%")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(115.))
                    .top(px(32.))
                    .text_size(px(13.))
                    .text_color(rgb(muted))
                    .child(limit),
            )
            .child(
                div()
                    .absolute()
                    .left(px(115.))
                    .top(px(57.))
                    .text_size(px(13.))
                    .text_color(rgb(muted))
                    .child(pace),
            );

        let limits_tab = div()
            .absolute()
            .top(px(332.))
            .left(px(22.))
            .w(px(396.))
            .h(px(35.))
            .rounded_lg()
            .bg(rgb(if self.dark { 0x444444 } else { 0xe5e5e5 }))
            .flex()
            .items_center()
            .px(px(16.))
            .cursor_pointer()
            .text_sm()
            .text_color(rgb(foreground))
            .child("Limits")
            .id("limits-tab")
            .on_click(cx.listener(|this, _, _, cx| {
                this.tab = 0;
                cx.notify();
            }));

        let fraction = amount as f32 / 100.0;
        let detail = div()
            .absolute()
            .top(px(380.))
            .left(px(20.))
            .w(px(400.))
            .h(px(172.))
            .rounded_lg()
            .bg(rgb(if self.dark { 0x2e2e2e } else { 0xf4f4f4 }))
            .child(
                div()
                    .absolute()
                    .left(px(15.))
                    .top(px(12.))
                    .text_size(px(13.))
                    .text_color(rgb(foreground))
                    .child("Rate limits"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(15.))
                    .top(px(35.))
                    .text_size(px(12.))
                    .text_color(rgb(muted))
                    .child("Weekly"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(230.))
                    .top(px(36.))
                    .text_size(px(10.))
                    .text_color(rgb(muted))
                    .child("resets 18 Oct"),
            )
            .child(
                div()
                    .absolute()
                    .left(px(350.))
                    .top(px(35.))
                    .text_size(px(12.))
                    .text_color(rgb(foreground))
                    .child(format!("{amount}%")),
            )
            .child(
                div()
                    .absolute()
                    .left(px(15.))
                    .top(px(59.))
                    .w(px(365.))
                    .h(px(8.))
                    .rounded_full()
                    .bg(rgb(track))
                    .child(div().h_full().w(px(365. * fraction)).bg(rgb(GREEN))),
            );

        let footer = div()
            .absolute()
            .top(px(574.))
            .left(px(22.))
            .w(px(396.))
            .h(px(16.))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(11.))
            .text_color(rgb(muted))
            .child(div().child("Resets 18 Oct (7 days)"))
            .child(div().child("Updated 3 min ago"));

        div()
            .track_focus(&self.focus)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
                    window.focus(&this.focus);
                    let x = f32::from(event.position.x);
                    let y = f32::from(event.position.y);
                    if (5.0..97.0).contains(&y) {
                        let width = (430.0 - 16.0) / 3.0;
                        for (index, (provider, _)) in ACCOUNTS.iter().enumerate() {
                            let left = 5.0 + index as f32 * (width + 8.0);
                            if (left..left + width).contains(&x) {
                                this.provider = *provider;
                                cx.notify();
                                break;
                            }
                        }
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                let delta = match event.delta {
                    ScrollDelta::Pixels(point) => f32::from(point.y),
                    ScrollDelta::Lines(point) => point.y,
                };
                if delta != 0.0 {
                    let index = account_order(this.provider);
                    let next = if delta > 0.0 {
                        (index + ACCOUNTS.len() - 1) % ACCOUNTS.len()
                    } else {
                        (index + 1) % ACCOUNTS.len()
                    };
                    this.provider = ACCOUNTS[next].0;
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.quit();
                }
            }))
            .size_full()
            .bg(rgb(background))
            .child(cards)
            .child(selector_mark)
            .child(header_row)
            .child(summary)
            .child(limits_tab)
            .child(detail)
            .child(footer)
    }
}

fn main() {
    let _ = fixture::DAYS;
    let mut dark = true;
    let mut auto_close = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--light" => dark = false,
            "--auto-close" => auto_close = args.next().and_then(|value| value.parse::<u64>().ok()),
            _ => {}
        }
    }
    if let Some(seconds) = auto_close {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(seconds));
            std::process::exit(0);
        });
    }
    Application::new().run(move |cx: &mut App| {
        cx.bind_keys([KeyBinding::new("escape", ClosePanel, None)]);
        cx.on_action(|_: &ClosePanel, cx| cx.quit());
        let bounds = Bounds::centered(None, size(px(440.0), px(606.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: None,
                kind: WindowKind::Normal,
                window_decorations: Some(WindowDecorations::Client),
                focus: true,
                is_movable: false,
                is_resizable: false,
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus);
                    UsagePanel {
                        provider: 3,
                        tab: 0,
                        dark,
                        focus,
                    }
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
