// Embeds the Windows version resource and app icon. Explorer and Settings
// show FileDescription as the app name (for example in the tray icon list).

#[cfg(windows)]
#[allow(dead_code, unused_imports)]
#[path = "src/icon.rs"]
mod icon;

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=src/icon.rs");
        let ico = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("tokenkeeper.ico");
        std::fs::write(&ico, app_icon()).expect("failed to write app icon");

        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().unwrap())
            .set("FileDescription", "TokenKeeper")
            .set("ProductName", "TokenKeeper")
            .set("InternalName", "tokenkeeper")
            .set("OriginalFilename", "tokenkeeper.exe")
            .set("LegalCopyright", "MIT License");
        res.compile().expect("failed to embed Windows version resource");
    }
}

/// The tray ring as a multi-size ICO with PNG entries. Mid-grey track and
/// tick so the icon reads on both light and dark backgrounds.
#[cfg(windows)]
fn app_icon() -> Vec<u8> {
    const SIZES: [u32; 8] = [16, 20, 24, 32, 40, 48, 64, 256];
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&size| {
            let rgba = icon::renderer::render_with(size, Some((0.65, 0.5)), (0x6E6E6E, 1.0), (0x8C8C8C, 0.45));
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, size, size);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().expect("png header");
            writer.write_image_data(&rgba).expect("png data");
            writer.finish().expect("png finish");
            out
        })
        .collect();

    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&size, image) in SIZES.iter().zip(&images) {
        let dim = if size >= 256 { 0 } else { size as u8 };
        ico.extend_from_slice(&[dim, dim, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // colour planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        ico.extend_from_slice(&(image.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in &images {
        ico.extend_from_slice(image);
    }
    ico
}
