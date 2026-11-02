//! Embeds the application icon and version metadata into the Windows executable.

fn main() {
    // Only embed resources when compiling for Windows (allows cross checks).
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        if std::path::Path::new("assets/app.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/app.ico");
            res.set("FileDescription", "escape-proxy");
            res.set("ProductName", "escape-proxy");
            res.set("FileVersion", env!("CARGO_PKG_VERSION"));
            res.set("ProductVersion", env!("CARGO_PKG_VERSION"));
            if let Err(e) = res.compile() {
                // A missing resource compiler must not break the build; the icon
                // is cosmetic and the binary still works without it.
                println!("cargo:warning=failed to embed windows resources: {e}");
            }
        } else {
            println!("cargo:warning=assets/app.ico missing; building without embedded icon");
        }
        println!("cargo:rerun-if-changed=assets/app.ico");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
