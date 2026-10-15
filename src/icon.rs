//! Software renderer for the tray icon: a progress ring with a pace tick.
//! Shapes are anti-aliased via 4x4 supersampling so the icon stays crisp at
//! every DPI-specific tray size (16, 20, 24, 32 px) without extra crates.

/// Usage colour as 0xRRGGBB: green, amber from 80%, red once the included
/// allowance is used up.
pub fn level_rgb(used: f32) -> u32 {
    if used >= 1.0 {
        0xF04E4E
    } else if used >= 0.8 {
        0xF0B030
    } else {
        0x3CC878
    }
}

pub use renderer::render;

pub(crate) mod renderer {
    use super::level_rgb;
    use std::f32::consts::TAU;
    #[derive(Clone, Copy)]
    struct Rgba(f32, f32, f32, f32);

    impl Rgba {
        fn hex(rgb: u32, a: f32) -> Self {
            Rgba(
                ((rgb >> 16) & 0xff) as f32 / 255.0,
                ((rgb >> 8) & 0xff) as f32 / 255.0,
                (rgb & 0xff) as f32 / 255.0,
                a,
            )
        }
    }

    struct Canvas {
        size: u32,
        /// Premultiplied RGBA.
        px: Vec<[f32; 4]>,
    }

    const SS: u32 = 4;

    impl Canvas {
        fn new(size: u32) -> Self {
            Canvas {
                size,
                px: vec![[0.0; 4]; (size * size) as usize],
            }
        }

        /// Source-over fill of every pixel where `inside(x, y)` holds, with
        /// coordinates in pixel units (0..size), sampled on a 4x4 grid.
        fn fill(&mut self, color: Rgba, inside: impl Fn(f32, f32) -> bool) {
            let n = (SS * SS) as f32;
            for py in 0..self.size {
                for px in 0..self.size {
                    let mut hits = 0u32;
                    for sy in 0..SS {
                        for sx in 0..SS {
                            let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
                            let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
                            if inside(x, y) {
                                hits += 1;
                            }
                        }
                    }
                    if hits == 0 {
                        continue;
                    }
                    let a = color.3 * hits as f32 / n;
                    let d = &mut self.px[(py * self.size + px) as usize];
                    let inv = 1.0 - a;
                    d[0] = color.0 * a + d[0] * inv;
                    d[1] = color.1 * a + d[1] * inv;
                    d[2] = color.2 * a + d[2] * inv;
                    d[3] = a + d[3] * inv;
                }
            }
        }

        fn into_rgba8(self) -> Vec<u8> {
            let mut out = Vec::with_capacity(self.px.len() * 4);
            for [r, g, b, a] in self.px {
                let un = |c: f32| {
                    if a > 0.0 {
                        (c / a).clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                };
                out.extend_from_slice(&[
                    (un(r) * 255.0).round() as u8,
                    (un(g) * 255.0).round() as u8,
                    (un(b) * 255.0).round() as u8,
                    (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                ]);
            }
            out
        }
    }

    /// Clockwise angle from 12 o'clock, as a fraction of a full turn (0..1).
    fn turn_fraction(dx: f32, dy: f32) -> f32 {
        let a = dx.atan2(-dy);
        (if a < 0.0 { a + TAU } else { a }) / TAU
    }

    /// Render the tray icon as RGBA8. `usage` is `(used, elapsed)`: the fraction
    /// of the included allowance consumed (may exceed 1.0) and the fraction of
    /// the billing cycle elapsed, drawn as the pace tick. `None` draws just the
    /// empty track, for signed-out or loading states.
    pub fn render(size: u32, usage: Option<(f32, f32)>, light_taskbar: bool) -> Vec<u8> {
        if light_taskbar {
            render_with(size, usage, (0x1A1A1A, 1.0), (0x000000, 0.18))
        } else {
            render_with(size, usage, (0xFFFFFF, 1.0), (0xFFFFFF, 0.25))
        }
    }

    /// `render` with explicit `(0xRRGGBB, alpha)` colours for the pace tick
    /// and the empty track. `build.rs` uses it for the exe icon (as `icon::renderer::render_with`).
    pub fn render_with(
        size: u32,
        usage: Option<(f32, f32)>,
        tick: (u32, f32),
        track: (u32, f32),
    ) -> Vec<u8> {
        let s = size as f32;
        let c = s / 2.0;
        let fg = Rgba::hex(tick.0, tick.1);
        let track = Rgba::hex(track.0, track.1);
        let mut cv = Canvas::new(size);

        let r_out = c - 0.5;
        let r_in = r_out - (s * 0.22).max(2.5);
        let annulus = move |x: f32, y: f32| {
            let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
            d >= r_in && d <= r_out
        };
        cv.fill(track, annulus);
        let Some((used, elapsed)) = usage else {
            return cv.into_rgba8();
        };
        let fill_frac = used.clamp(0.0, 1.0);
        cv.fill(Rgba::hex(level_rgb(used), 1.0), move |x, y| {
            annulus(x, y) && turn_fraction(x - c, y - c) <= fill_frac
        });

        let half_width = 0.6 / r_out.max(1.0) / TAU * 2.0;
        let e = elapsed.clamp(0.0, 1.0);
        cv.fill(fg, move |x, y| {
            let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt();
            let t = turn_fraction(x - c, y - c);
            let dt = (t - e).abs().min(1.0 - (t - e).abs());
            d >= r_in - 1.0 && d <= r_out && dt <= half_width
        });
        cv.into_rgba8()
    }
}
