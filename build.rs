//! Gives the Windows program its icon and the name Windows shows for it.
//! On other systems there is nothing to do.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "End Grain Calc")
            .set("FileDescription", "End Grain Calc")
            .set("CompanyName", "TEA JAY HAN")
            .set("LegalCopyright", "\u{a9} 2026 TEA JAY HAN. MIT licence.");
        if let Err(e) = res.compile() {
            // A program without its icon still works; say so and carry on.
            println!("cargo:warning=Couldn't add the Windows icon: {e}");
        }
    }
}
