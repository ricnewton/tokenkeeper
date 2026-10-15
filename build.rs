// Embeds the Windows version resource, app icon and manifest. Explorer and Settings
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
            .set_manifest(MANIFEST)
            .set("FileDescription", "TokenKeeper")
            .set("ProductName", "TokenKeeper")
            .set("InternalName", "tokenkeeper")
            .set("OriginalFilename", "tokenkeeper.exe")
            .set("LegalCopyright", "MIT License");
        res.compile().expect("failed to embed Windows version resource");
    }
}

/// Windows 10/11 compatibility, per-monitor v2 DPI awareness (also set at
/// startup in `main_windows.rs`) and no elevation.
#[cfg(windows)]
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#;

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
