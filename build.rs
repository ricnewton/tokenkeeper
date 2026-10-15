// Embeds the Windows version resource. Explorer and Settings show
// FileDescription as the app name (for example in the tray icon list).
fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set("FileDescription", "TokenKeeper")
            .set("ProductName", "TokenKeeper")
            .set("InternalName", "tokenkeeper")
            .set("OriginalFilename", "tokenkeeper.exe")
            .set("LegalCopyright", "MIT License");
        res.compile().expect("failed to embed Windows version resource");
    }
}
