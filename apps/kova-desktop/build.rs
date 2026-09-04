fn main() {
    // Slint's generated item tree exceeds MSVC's 1 MiB main-thread stack
    // in debug builds. Reserve space without changing the UI thread model.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=kova-desktop=/STACK:8388608");
    }
    slint_build::compile("ui/main.slint").unwrap();
}
