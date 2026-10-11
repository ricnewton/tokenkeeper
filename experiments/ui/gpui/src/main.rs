#[path = "../../shared/fixture.rs"]
mod fixture;

use fixture::{DAYS, PROVIDERS};
use gpui::{
    actions, div, prelude::*, px, rgb, size, App, Application, Bounds, Context, KeyBinding, Render,
    SharedString, Window, WindowBounds, WindowOptions,
};
use std::{env, time::Duration};

actions!(usage_panel, [ClosePanel]);

struct UsagePanel {
    provider: usize,
    tab: usize,
    dark: bool,
    focus: gpui::FocusHandle,
}

impl UsagePanel {
    fn surface(&self) -> u32 {
        if self.dark {
            0x171922
        } else {
            0xf4f5f8
        }
    }
    fn text(&self) -> u32 {
        if self.dark {
            0xf0f1f6
        } else {
            0x20232c
        }
    }
    fn muted(&self) -> u32 {
        if self.dark {
            0x969aa8
        } else {
            0x6d7280
        }
    }
}

impl Render for UsagePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = PROVIDERS[self.provider];
        let background = self.surface();
        let foreground = self.text();
        let muted = self.muted();
        let active_tab = self.tab;

        let mut providers = div().flex().gap_2().w_full();
        for (index, provider) in PROVIDERS.iter().enumerate() {
            let selected = index == self.provider;
            let name: SharedString = provider.name.into();
            let color = (u32::from(provider.color[0]) << 16)
                | (u32::from(provider.color[1]) << 8)
                | u32::from(provider.color[2]);
            providers = providers.child(
                div()
                    .id(("provider", index))
                    .flex_1()
                    .px_2()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(if selected {
                        color
                    } else if self.dark {
                        0x282b36
                    } else {
                        0xe6e8ed
                    }))
                    .text_color(rgb(if selected { 0xffffff } else { foreground }))
                    .text_xs()
                    .text_center()
                    .cursor_pointer()
                    .child(name)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.provider = index;
                        cx.notify();
                    })),
            );
        }

        let mut tabs = div().flex().gap_2().w_full();
        for (index, label) in ["Limits", "This cycle", "History"].iter().enumerate() {
            let selected = index == active_tab;
            let name: SharedString = (*label).into();
            tabs = tabs.child(
                div()
                    .id(("tab", index))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(if selected { 0x343748 } else { background }))
                    .text_color(rgb(if selected { 0xffffff } else { muted }))
                    .text_sm()
                    .cursor_pointer()
                    .child(name)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = index;
                        cx.notify();
                    })),
            );
        }

        let theme_label = if self.dark {
            "☼  Light theme"
        } else {
            "☾  Dark theme"
        };
        let mut chart = div().flex().items_end().gap_2().h(px(112.)).w_full();
        for value in DAYS {
            chart = chart.child(
                div()
                    .flex_1()
                    .h(px(f32::from(value) * 0.92))
                    .rounded_sm()
                    .bg(rgb((u32::from(p.color[0]) << 16)
                        | (u32::from(p.color[1]) << 8)
                        | u32::from(p.color[2]))),
            );
        }
        let ring_color =
            (u32::from(p.color[0]) << 16) | (u32::from(p.color[1]) << 8) | u32::from(p.color[2]);
        let ring = div()
            .size(px(82.))
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::canvas(
                    move |_, _, _| (),
                    move |bounds, _, window, _| {
                        for (fraction, color) in [
                            (1.0_f32, 0x444754_u32),
                            (p.percent as f32 / 100.0, ring_color),
                        ] {
                            let mut path = gpui::PathBuilder::stroke(px(6.));
                            let center = bounds.center();
                            for i in 0..=64 {
                                let angle = -std::f32::consts::FRAC_PI_2
                                    + std::f32::consts::TAU * fraction * i as f32 / 64.0;
                                let point = gpui::point(
                                    center.x + px(angle.cos() * 35.),
                                    center.y + px(angle.sin() * 35.),
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
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(format!("{}%", p.percent)),
            );
        div()
            .track_focus(&self.focus)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, _| window.focus(&this.focus)),
            )
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.quit();
                }
            }))
            .size_full()
            .bg(rgb(background))
            .text_color(rgb(foreground))
            .p_5()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("◈  Usage"),
                    )
                    .child(
                        div()
                            .id("theme-toggle")
                            .text_sm()
                            .text_color(rgb(muted))
                            .cursor_pointer()
                            .child(theme_label)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.dark = !this.dark;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x67d5a3))
                    .child("●  All systems normal"),
            )
            .child(providers)
            .child(tabs)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_lg()
                    .bg(rgb(if self.dark { 0x20232e } else { 0xffffff }))
                    .p_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(div().text_xs().text_color(rgb(muted)).child("THIS CYCLE"))
                            .child(
                                div()
                                    .text_3xl()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(p.used),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(rgb(muted))
                                    .child(format!("of {} · resets in 12 days", p.limit)),
                            ),
                    )
                    .child(ring),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_sm()
                            .child("Usage limit")
                            .child(format!("{}%", p.percent)),
                    )
                    .child(
                        div()
                            .h(px(8.))
                            .rounded_full()
                            .bg(rgb(if self.dark { 0x333743 } else { 0xe1e4eb }))
                            .child(
                                div()
                                    .h_full()
                                    .w(px(f32::from(p.percent) * 3.35))
                                    .rounded_full()
                                    .bg(rgb((u32::from(p.color[0]) << 16)
                                        | (u32::from(p.color[1]) << 8)
                                        | u32::from(p.color[2]))),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .text_sm()
                    .child(match self.tab {
                        0 => "Recent activity",
                        1 => "Cycle activity",
                        _ => "Usage history",
                    })
                    .child(div().text_xs().text_color(rgb(muted)).child("Last 12 days")),
            )
            .child(chart)
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(muted))
                    .child("All values are sample data"),
            )
    }
}

fn main() {
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
        let bounds = Bounds::centered(None, size(px(440.0), px(600.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                focus: true,
                is_resizable: false,
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus);
                    UsagePanel {
                        provider: 0,
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
