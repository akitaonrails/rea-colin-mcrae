fn main() {
    // The DLL needs no C runtime start-up: link without its start files
    // (see `DllMainCRTStartup` in lib.rs).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-cdylib-link-arg=-nostartfiles");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
    }
}
