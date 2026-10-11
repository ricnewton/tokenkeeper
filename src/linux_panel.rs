//! Custom Iced rendering of the provider-neutral usage flyout.
use crate::{
    icon,
    model::{Detail, ProviderId, Snapshot, Unit, View},
    time,
};
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Size, Theme};
use std::cell::Cell;

#[derive(Debug, Clone)]
pub enum Action {
    Click(f64, f64),
    Scroll(i32),
}

pub struct Panel {
    pub slots: Vec<(ProviderId, View)>,
    pub active: usize,
    pub tab: usize,
    pub light: bool,
    pub generation: u64,
}

pub fn visible_providers(slots: &[(ProviderId, View)], active: usize) -> Vec<usize> {
    let mut visible: Vec<_> = slots
        .iter()
        .enumerate()
        .filter(|(_, (_, v))| v.snapshot().is_some() || matches!(v, View::SigningIn { .. }))
        .map(|(i, _)| i)
        .collect();
    if active < slots.len() && !visible.contains(&active) {
        visible.push(active);
        visible.sort_unstable();
    }
    visible
}

pub fn detail_height(s: &Snapshot, tab: usize) -> f32 {
    if s.details.get(tab) == Some(&Detail::Limits) {
        (50.0 + s.meters.len() as f32 * 38.0).max(172.0)
    } else {
        172.0
    }
}

pub fn content_height(slots: &[(ProviderId, View)], active: usize, tab: usize) -> f32 {
    slots
        .get(active)
        .and_then(|(_, v)| v.snapshot())
        .map_or(610.0, |s| 380.0 + detail_height(s, tab) + 58.0)
        - if visible_providers(slots, active).len() == 1 {
            155.0
        } else {
            0.0
        }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Hit {
    Provider(usize),
    Tab(usize),
    Usage,
    None,
}
pub fn hit_test(x: f64, y: f64, providers: &[usize], details: usize) -> Hit {
    if !(0.0..440.0).contains(&x) || y < 0.0 {
        return Hit::None;
    }
    if y < 102.0 && providers.len() > 1 {
        return Hit::Provider(providers[(x / (440.0 / providers.len() as f64)) as usize]);
    }
    let single = providers.len() == 1;
    let detail_y = y + if single { 155.0 } else { 0.0 };
    if (330.0..370.0).contains(&detail_y) && (20.0..420.0).contains(&x) && details > 0 {
        return Hit::Tab(((x - 20.0) / (400.0 / details as f64)) as usize);
    }
    let header_y = y + if single { 95.0 } else { 0.0 };
    if (115.0..155.0).contains(&header_y) && x > 345.0 {
        return Hit::Usage;
    }
    Hit::None
}

#[derive(Default)]
pub struct PanelState {
    cache: canvas::Cache,
    key: Cell<Option<(u64, i64, usize, usize, bool)>>,
}
impl canvas::Program<Action> for Panel {
    type State = PanelState;
    fn update(
        &self,
        _: &mut PanelState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Action>> {
        let p = cursor.position_in(bounds)?;
        match event {
            iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                Some(canvas::Action::publish(Action::Click(p.x as f64, p.y as f64)).and_capture())
            }
            iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta })
                if p.y < 102.0 && visible_providers(&self.slots, self.active).len() > 1 =>
            {
                let y = match delta {
                    iced::mouse::ScrollDelta::Lines { y, .. }
                    | iced::mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                if y == 0.0 {
                    None
                } else {
                    Some(
                        canvas::Action::publish(Action::Scroll(if y > 0.0 { 1 } else { -1 }))
                            .and_capture(),
                    )
                }
            }
            _ => None,
        }
    }
    fn mouse_interaction(
        &self,
        _: &PanelState,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        let providers = visible_providers(&self.slots, self.active);
        let details = self
            .slots
            .get(self.active)
            .and_then(|(_, v)| v.snapshot())
            .map_or(0, |s| s.details.len());
        if cursor
            .position_in(bounds)
            .is_some_and(|p| hit_test(p.x as f64, p.y as f64, &providers, details) != Hit::None)
        {
            iced::mouse::Interaction::Pointer
        } else {
            iced::mouse::Interaction::default()
        }
    }
    fn draw(
        &self,
        state: &PanelState,
        renderer: &iced::Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let key = (
            self.generation,
            time::now_ms() / 60_000,
            self.active,
            self.tab,
            self.light,
        );
        if state.key.get() != Some(key) {
            state.cache.clear();
            state.key.set(Some(key));
        }
        let geometry = state.cache.draw(renderer, bounds.size(), |mut f| {
            f.fill_rectangle(
                Point::ORIGIN,
                bounds.size(),
                Color::from_rgb(
                    if self.light { 0.976 } else { 0.169 },
                    if self.light { 0.976 } else { 0.169 },
                    if self.light { 0.976 } else { 0.169 },
                ),
            );
            let providers = visible_providers(&self.slots, self.active);
            if providers.len() > 1 {
                let w = 440.0 / providers.len() as f32;
                for (column, &i) in providers.iter().enumerate() {
                    let (id, view) = &self.slots[i];
                    let x = column as f32 * w;
                    self.rounded(
                        &mut f,
                        x + 4.0,
                        5.0,
                        w - 8.0,
                        92.0,
                        9.0,
                        if i == self.active { 0.25 } else { 0.20 },
                    );
                    self.text(
                        &mut f,
                        x + 12.0,
                        26.0,
                        id.short_name(),
                        13.0,
                        if i == self.active { 0.98 } else { 0.75 },
                    );
                    if let Some(s) = view.snapshot() {
                        self.ring(
                            &mut f,
                            x + 35.0,
                            61.0,
                            17.0,
                            s.used_fraction(),
                            s.elapsed_fraction(),
                        );
                        self.text(
                            &mut f,
                            x + 59.0,
                            65.0,
                            &format!("{:.0}%", s.used_fraction() * 100.0),
                            14.0,
                            0.95,
                        );
                    } else {
                        self.text(&mut f, x + 12.0, 64.0, "Signed out", 11.0, 0.55);
                    }
                    if i == self.active {
                        self.rounded(&mut f, x + w / 2.0 - 7.0, 97.0, 14.0, 3.0, 2.0, 0.7);
                    }
                }
            }
            let Some((id, view)) = self.slots.get(self.active) else {
                return;
            };
            let single = providers.len() == 1;
            f.translate(iced::Vector::new(0.0, if single { -95.0 } else { 0.0 }));
            self.text(&mut f, 22.0, 137.0, id.name(), 17.0, 1.0);
            self.colored_text(&mut f, 361.0, 137.0, "Usage >", 12.0, rgb(0x58c7eb));
            if let Some(s) = view.snapshot() {
                self.rounded(&mut f, 252.0, 116.0, 100.0, 27.0, 13.0, 0.23);
                self.text(&mut f, 261.0, 134.0, &s.plan, 10.0, 0.75);
                self.ring(
                    &mut f,
                    75.0,
                    205.0,
                    43.0,
                    s.used_fraction(),
                    s.elapsed_fraction(),
                );
                self.text(
                    &mut f,
                    55.0,
                    212.0,
                    &format!("{:.0}%", s.used_fraction() * 100.0),
                    17.0,
                    1.0,
                );
                self.text(&mut f, 145.0, 184.0, &s.fmt(s.used), 28.0, 1.0);
                self.text(&mut f, 145.0, 207.0, &s.of_text(), 13.0, 0.62);
                self.text(&mut f, 145.0, 232.0, &s.pace_text(), 13.0, 0.8);
                self.draw_combined(&mut f);
                if single {
                    f.translate(iced::Vector::new(0.0, -60.0));
                }
                self.rounded(&mut f, 20.0, 330.0, 400.0, 38.0, 8.0, 0.18);
                for (i, d) in s.details.iter().enumerate() {
                    let w = 400.0 / s.details.len() as f32;
                    let x = 20.0 + i as f32 * w;
                    if i == self.tab {
                        self.rounded(&mut f, x + 2.0, 332.0, w - 4.0, 34.0, 7.0, 0.27);
                    }
                    self.text(
                        &mut f,
                        x + 18.0,
                        355.0,
                        d.label(),
                        13.0,
                        if i == self.tab { 1.0 } else { 0.6 },
                    );
                }
                let h = detail_height(s, self.tab);
                self.rounded(&mut f, 20.0, 380.0, 400.0, h, 9.0, 0.18);
                self.draw_detail(&mut f, s);
                let footer = 380.0 + h + 34.0;
                self.text(&mut f, 22.0, footer, &s.resets_text(), 11.0, 0.56);
                if matches!(view, View::Ready { error: Some(_), .. }) {
                    self.colored_text(
                        &mut f,
                        275.0,
                        footer,
                        "Update failed, retrying",
                        11.0,
                        rgb(0xf0b030),
                    );
                } else {
                    self.text(
                        &mut f,
                        315.0,
                        footer,
                        &format!("Updated {}", time::ago(s.fetched_ms)),
                        11.0,
                        0.56,
                    );
                }
            } else {
                let msg = match view {
                    View::Loading => "Loading usage…",
                    View::SignedOut(_) => "Sign in from the tray menu",
                    View::SigningIn { code: Some(_) } => "Enter the copied code in your browser",
                    View::SigningIn { .. } => "Complete sign-in in your browser",
                    View::Failed(_) => "Could not load usage",
                    _ => "",
                };
                self.text(&mut f, 22.0, 200.0, msg, 15.0, 0.75);
                if let View::SignedOut(Some(error)) | View::Failed(error) = view {
                    self.colored_text(
                        &mut f,
                        22.0,
                        228.0,
                        &error.chars().take(65).collect::<String>(),
                        11.0,
                        rgb(0xf0b030),
                    );
                }
                if let View::SigningIn { code: Some(code) } = view {
                    self.text(&mut f, 22.0, 248.0, code, 26.0, 1.0);
                }
            }
        });
        vec![geometry]
    }
}

impl Panel {
    fn gray(&self, v: f32) -> Color {
        let v = if self.light { 1.0 - v } else { v };
        Color::from_rgb(v, v, v)
    }
    fn rounded(&self, f: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32, v: f32) {
        let color = if self.light && v <= 0.5 {
            let v = 1.0 - v * 0.25;
            Color::from_rgb(v, v, v)
        } else {
            self.gray(v)
        };
        f.fill(
            &Path::rounded_rectangle(
                Point::new(x, y),
                Size::new(w, h),
                iced::border::Radius::from(r),
            ),
            color,
        );
    }
    fn text(&self, f: &mut Frame, x: f32, y: f32, s: &str, size: f32, v: f32) {
        self.colored_text(f, x, y, s, size, self.gray(v));
    }
    fn colored_text(&self, f: &mut Frame, x: f32, y: f32, s: &str, size: f32, color: Color) {
        f.fill_text(Text {
            content: s.into(),
            position: Point::new(x, y - size),
            size: iced::Pixels(size),
            color,
            line_height: iced::widget::text::LineHeight::Absolute(iced::Pixels(size + 2.0)),
            ..Text::default()
        });
    }
    fn ring(&self, f: &mut Frame, x: f32, y: f32, r: f32, used: f32, elapsed: f32) {
        f.stroke(
            &Path::circle(Point::new(x, y), r),
            Stroke::default()
                .with_width(5.0)
                .with_color(self.gray(0.22)),
        );
        if used > 0.0 {
            let arc = Path::new(|b| {
                b.arc(canvas::path::Arc {
                    center: Point::new(x, y),
                    radius: r,
                    start_angle: iced::Radians(-std::f32::consts::FRAC_PI_2),
                    end_angle: iced::Radians(
                        -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * used.min(1.0),
                    ),
                })
            });
            f.stroke(
                &arc,
                Stroke::default()
                    .with_width(5.0)
                    .with_color(rgb(icon::level_rgb(used))),
            );
        }
        let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * elapsed;
        f.stroke(
            &Path::line(
                Point::new(x + (r - 4.0) * a.cos(), y + (r - 4.0) * a.sin()),
                Point::new(x + (r + 4.0) * a.cos(), y + (r + 4.0) * a.sin()),
            ),
            Stroke::default().with_width(2.0).with_color(self.gray(0.9)),
        );
    }
    fn bar(&self, f: &mut Frame, x: f32, y: f32, w: f32, used: f32) {
        self.rounded(f, x, y, w, 8.0, 4.0, 0.3);
        f.fill_rectangle(
            Point::new(x, y),
            Size::new(w * used.clamp(0.0, 1.0), 8.0),
            rgb(icon::level_rgb(used)),
        );
    }
    fn draw_combined(&self, f: &mut Frame) {
        let dollars: Vec<_> = self
            .slots
            .iter()
            .filter_map(|(_, v)| v.snapshot())
            .filter(|s| s.unit == Unit::Cents && s.limit > 0)
            .collect();
        if dollars.len() < 2 {
            return;
        }
        let used: u32 = dollars.iter().map(|s| s.used).sum();
        let limit: u32 = dollars.iter().map(|s| s.limit).sum();
        self.text(f, 22.0, 273.0, "All dollar tools", 12.0, 0.7);
        self.text(
            f,
            255.0,
            273.0,
            &format!(
                "{} / {}",
                crate::model::dollars(used),
                crate::model::dollars(limit)
            ),
            11.0,
            0.8,
        );
        self.rounded(f, 22.0, 284.0, 396.0, 11.0, 5.0, 0.19);
        let mut x = 22.0;
        for s in dollars {
            let w = 396.0 * s.used.min(s.limit) as f32 / limit as f32;
            let color = match s.provider {
                ProviderId::Cursor => 0x58c7eb,
                ProviderId::Copilot => 0x8957e5,
                ProviderId::Claude => 0xd97757,
                ProviderId::Codex => 0x10a37f,
                ProviderId::OpenCode => 0x6e9ee8,
            };
            f.fill_rectangle(Point::new(x, 284.0), Size::new(w, 11.0), rgb(color));
            x += w;
        }
    }
    fn draw_detail(&self, f: &mut Frame, s: &Snapshot) {
        let detail = s.details.get(self.tab).copied().unwrap_or(Detail::Today);
        if detail == Detail::Limits {
            self.text(f, 35.0, 407.0, "Rate limits", 13.0, 0.95);
            for (i, m) in s.meters.iter().enumerate() {
                let y = 432.0 + i as f32 * 38.0;
                self.text(f, 35.0, y, &m.label, 12.0, 0.8);
                if let Some(reset) = m.resets_ms {
                    self.text(
                        f,
                        250.0,
                        y,
                        &format!("resets {}", time::day_month(reset)),
                        10.0,
                        0.58,
                    );
                }
                self.text(
                    f,
                    370.0,
                    y,
                    &format!("{:.0}%", m.fraction * 100.0),
                    12.0,
                    0.9,
                );
                self.bar(f, 35.0, y + 7.0, 365.0, m.fraction);
            }
            return;
        }
        let (values, labels, title): (Vec<Option<u32>>, Vec<String>, &str) = match detail {
            Detail::Today => (
                s.hourly.clone(),
                (0..s.hourly.len()).map(|i| format!("{i}:00")).collect(),
                "Today by hour",
            ),
            Detail::Cycle => (s.daily.clone(), s.daily_labels.clone(), "Daily this cycle"),
            Detail::History => (
                s.history.iter().copied().map(Some).collect(),
                s.history_labels.clone(),
                "Monthly totals",
            ),
            Detail::Limits => unreachable!(),
        };
        self.text(f, 35.0, 408.0, title, 13.0, 0.95);
        if values.is_empty() {
            self.text(f, 85.0, 480.0, s.empty_note, 13.0, 0.55);
            return;
        }
        let max = values.iter().filter_map(|v| *v).max().unwrap_or(1).max(1) as f32;
        let w = 365.0 / values.len() as f32;
        if detail == Detail::Cycle && s.limit > 0 {
            let budget = s.limit as f32 / s.cycle_days();
            let y = 524.0 - (budget / max * 97.0).min(100.0);
            // Dash geometry avoids renderer-specific dash state and preserves the budget line.
            let mut x = 35.0;
            while x < 400.0 {
                f.stroke(
                    &Path::line(Point::new(x, y), Point::new((x + 3.0).min(400.0), y)),
                    Stroke::default()
                        .with_width(1.0)
                        .with_color(self.gray(0.58)),
                );
                x += 6.0;
            }
            self.text(
                f,
                310.0,
                408.0,
                &format!("budget {}/day", s.fmt(budget as u32)),
                10.0,
                0.62,
            );
        }
        for (i, v) in values.iter().enumerate() {
            if let Some(v) = v {
                let h = *v as f32 / max * 97.0;
                f.fill_rectangle(
                    Point::new(35.0 + i as f32 * w, 524.0 - h),
                    Size::new((w - 2.0).max(1.0), h),
                    rgb(if *v > s.limit && detail == Detail::History {
                        0xc65353
                    } else {
                        0x5bc4ed
                    }),
                );
            }
        }
        for i in [0, values.len() / 2, values.len() - 1] {
            if let Some(label) = labels.get(i) {
                self.text(f, 35.0 + i as f32 * w, 544.0, label, 10.0, 0.55);
            }
        }
    }
}
fn rgb(value: u32) -> Color {
    Color::from_rgb8((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    #[test]
    fn meter_rows_and_footer_fit_canvas_for_single_and_multiple_providers() {
        let mut s = Snapshot::new(ProviderId::Codex, Unit::Basis);
        s.details = vec![Detail::Limits];
        for count in [0, 1, 3, 8, 20] {
            s.meters = (0..count)
                .map(|i| crate::model::Meter {
                    label: format!("limit {i}"),
                    fraction: 0.5,
                    resets_ms: None,
                })
                .collect();
            let slots = vec![
                (
                    ProviderId::Codex,
                    View::Ready {
                        snap: Rc::new(s.clone()),
                        error: None,
                    },
                ),
                (ProviderId::Cursor, View::SignedOut(None)),
            ];
            let h = content_height(&slots, 0, 0);
            let last_bar = if count == 0 {
                380.0
            } else {
                432.0 + (count - 1) as f32 * 38.0 + 15.0
            };
            assert!(last_bar - 155.0 < h - 58.0);
            let mut multi = slots;
            multi[1].1 = View::SigningIn { code: None };
            assert_eq!(content_height(&multi, 0, 0), h + 155.0);
        }
    }
    #[test]
    fn card_indices_and_single_provider_hit_offsets_match_drawing() {
        assert_eq!(hit_test(385.0, 60.0, &[0, 1, 3], 3), Hit::Provider(3));
        assert_eq!(hit_test(180.0, 345.0, &[0, 1, 3], 3), Hit::Tab(1));
        assert_eq!(hit_test(400.0, 345.0, &[0, 1, 3], 3), Hit::Tab(2));
        assert_eq!(hit_test(180.0, 190.0, &[3], 3), Hit::Tab(1));
        assert_eq!(hit_test(400.0, 40.0, &[3], 1), Hit::Usage);
        assert_eq!(hit_test(220.0, 60.0, &[3], 1), Hit::None);
        assert_eq!(hit_test(440.0, 60.0, &[0, 1], 1), Hit::None);
        assert_eq!(hit_test(10.0, 345.0, &[0, 1], 3), Hit::None);
        assert_eq!(hit_test(400.0, 800.0, &[0, 1], 3), Hit::None);
    }
    #[test]
    fn signing_in_and_active_signed_out_remain_visible() {
        let slots = vec![
            (ProviderId::Cursor, View::SignedOut(None)),
            (ProviderId::Claude, View::SigningIn { code: None }),
            (ProviderId::Codex, View::Loading),
        ];
        assert_eq!(visible_providers(&slots, 2), vec![1, 2]);
        assert_eq!(visible_providers(&slots, 0), vec![0, 1]);
        assert_eq!(visible_providers(&[], 0), Vec::<usize>::new());
    }
}
