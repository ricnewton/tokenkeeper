//! Dev tool: renders the tray icon at several usage levels and sizes into a
//! PNG contact sheet (`tokenkeeper.exe --icon-sheet out.png`).

use crate::icon;

const LEVELS: [f32; 6] = [0.05, 0.35, 0.6, 0.8, 0.95, 1.2];
const SIZES: [(u32, u32); 4] = [(16, 4), (20, 3), (24, 3), (32, 2)];
const ELAPSED: f32 = 0.55;
const CELL: u32 = 64;
const PAD: u32 = 12;

pub fn write(path: &str) -> std::io::Result<()> {
    let cols = LEVELS.len() as u32;
    let rows = (SIZES.len() * 2) as u32;
    let w = cols * (CELL + PAD) + PAD;
    let h = rows * (CELL + PAD) + PAD;
    let mut img = vec![0u8; (w * h * 4) as usize];

    for (ti, light) in [false, true].into_iter().enumerate() {
        let bg: [u8; 3] = if light { [0xF3, 0xF3, 0xF3] } else { [0x1F, 0x1F, 0x1F] };
        let band_y0 = ti as u32 * SIZES.len() as u32 * (CELL + PAD);
        let band_h = SIZES.len() as u32 * (CELL + PAD) + PAD;
        for y in band_y0..(band_y0 + band_h).min(h) {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                img[i..i + 4].copy_from_slice(&[bg[0], bg[1], bg[2], 255]);
            }
        }
        for (zi, &(size, scale)) in SIZES.iter().enumerate() {
            let row = (ti * SIZES.len() + zi) as u32;
            for (li, &used) in LEVELS.iter().enumerate() {
                let ox = PAD + li as u32 * (CELL + PAD);
                let oy = PAD + row * (CELL + PAD);
                let px = icon::render(size, Some((used, ELAPSED)), light);
                blit(&mut img, w, ox, oy, &px, size, scale);
            }
        }
    }
    std::fs::write(path, encode_png(w, h, &img))
}

/// Renders Glance and each Panel tab, dark row then light row, at 150% scale
/// (`tokenkeeper.exe --popup-sheet out.png`).
pub fn write_popups(path: &str, used: f32) -> windows::core::Result<()> {
    use crate::model::{ProviderId, View};
    use crate::popup::{self, Kind};
    use std::rc::Rc;
    let gfx = popup::Gfx::new()?;
    let full = Rc::new(crate::mock::simulated(used));
    let ready = View::Ready { snap: full.clone(), error: None };
    // What the real API gives before the per-event breakdown is wired up.
    let mut bare = (*full).clone();
    bare.today = None;
    bare.daily.clear();
    bare.hourly.clear();
    bare.history.clear();
    let bare = View::Ready { snap: Rc::new(bare), error: None };
    let failing = View::Ready { snap: full, error: Some("request to cursor.com failed: timeout".into()) };
    let copilot = View::Ready { snap: Rc::new(crate::mock::simulated_copilot(used)), error: None };
    let claude = View::Ready { snap: Rc::new(crate::mock::simulated_claude(used)), error: None };
    let claude_spend = View::Ready { snap: Rc::new(crate::mock::simulated_claude_spend(used)), error: None };
    let device = View::SigningIn { code: Some("A1B2-C3D4".into()) };
    let browser = View::SigningIn { code: None };

    use ProviderId::{Claude, Copilot, Cursor};
    type Case = (Kind, usize, Vec<(ProviderId, View)>, ProviderId);
    let one = |kind: Kind, tab: usize, id: ProviderId, v: &View| -> Case { (kind, tab, vec![(id, v.clone())], id) };
    let all = |claude_view: &View| -> Vec<(ProviderId, View)> {
        vec![(Cursor, ready.clone()), (Copilot, copilot.clone()), (Claude, claude_view.clone())]
    };
    let normal = vec![one(Kind::Glance, 0, Cursor, &ready), one(Kind::Panel, 0, Cursor, &ready), one(Kind::Panel, 1, Cursor, &ready), one(Kind::Panel, 2, Cursor, &ready)];
    let states = vec![one(Kind::Glance, 0, Cursor, &View::SignedOut(None)), one(Kind::Glance, 0, Cursor, &browser), one(Kind::Panel, 1, Cursor, &bare), one(Kind::Glance, 0, Cursor, &failing)];
    let opencode = View::Ready { snap: std::rc::Rc::new(crate::mock::simulated_opencode(0.38)), error: None };
    let others = vec![one(Kind::Glance, 0, Copilot, &copilot), one(Kind::Panel, 1, Copilot, &copilot), one(Kind::Glance, 0, Copilot, &device), one(Kind::Glance, 0, Claude, &claude), one(Kind::Panel, 0, Claude, &claude), one(Kind::Panel, 0, crate::model::ProviderId::OpenCode, &opencode)];
    let two = vec![(Cursor, ready.clone()), (Copilot, copilot.clone()), (Claude, View::SignedOut(None))];
    let multi: Vec<Case> = vec![
        (Kind::Glance, 0, all(&claude_spend), Cursor),
        (Kind::Glance, 0, two.clone(), Copilot),
        (Kind::Panel, 1, all(&claude), Cursor),
        (Kind::Glance, 0, all(&View::Loading), Cursor),
    ];
    let multi_light: Vec<Case> = vec![(Kind::Glance, 0, all(&claude), Cursor), (Kind::Panel, 1, two, Cursor)];
    let gap = 24u32;
    let mut rows = Vec::new();
    for (light, cases) in [(false, &normal), (true, &normal), (false, &states), (false, &others), (false, &multi), (true, &multi_light)] {
        let mut row = Vec::new();
        for (kind, tab, entries, active) in cases.iter() {
            row.push(popup::render_offscreen(&gfx, *kind, *tab, entries, *active, light, 144.0)?);
        }
        rows.push(row);
    }
    let w = rows.iter().map(|row| row.iter().map(|r| r.0 + gap).sum::<u32>()).max().unwrap_or(0) + gap;
    let mut row_tops = Vec::new();
    let mut h = gap;
    for row in &rows {
        row_tops.push(h);
        h += row.iter().map(|r| r.1).max().unwrap_or(0) + gap;
    }
    let mut img = vec![0u8; (w * h * 4) as usize];
    for px in img.chunks_mut(4) {
        px.copy_from_slice(&[0x1B, 0x3A, 0x5E, 255]);
    }
    for (ri, row) in rows.iter().enumerate() {
        let mut x = gap;
        for (pw, ph, buf) in row {
            let y = row_tops[ri];
            for yy in 0..*ph {
                let src = (yy * pw * 4) as usize;
                let dst = (((y + yy) * w + x) * 4) as usize;
                img[dst..dst + (*pw * 4) as usize].copy_from_slice(&buf[src..src + (*pw * 4) as usize]);
            }
            x += pw + gap;
        }
    }
    std::fs::write(path, encode_png(w, h, &img)).map_err(|e| windows::core::Error::new(windows::core::HRESULT(-1), e.to_string()))
}

fn blit(img: &mut [u8], w: u32, ox: u32, oy: u32, src: &[u8], size: u32, scale: u32) {
    for y in 0..size * scale {
        for x in 0..size * scale {
            let s = (((y / scale) * size + x / scale) * 4) as usize;
            let d = (((oy + y) * w + ox + x) * 4) as usize;
            let a = src[s + 3] as u32;
            for c in 0..3 {
                img[d + c] = ((src[s + c] as u32 * a + img[d + c] as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(((w * 4 + 1) * h) as usize);
    for row in rgba.chunks(w as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    // zlib stream made of uncompressed deflate blocks.
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(block) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = block.len() as u16;
        z.extend_from_slice(&len.to_le_bytes());
        z.extend_from_slice(&(!len).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
