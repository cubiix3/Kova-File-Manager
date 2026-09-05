fn main() {
    println!("cargo:rerun-if-changed=assets/kova.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/kova.ico")
            .set("ProductName", "Kova")
            .set("FileDescription", "Kova File Manager")
            .set("OriginalFilename", "kova-desktop.exe")
            .compile()
            .expect("compile Windows icon resources");
    }
    // Slint's generated item tree exceeds MSVC's 1 MiB main-thread stack
    // in debug builds. Reserve space without changing the UI thread model.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=kova-desktop=/STACK:8388608");
    }
    slint_build::compile("ui/main.slint").unwrap();
}
