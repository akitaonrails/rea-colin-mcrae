fn main() {
    // The DLL needs no C runtime start-up: link without its start files
    // (see `DllMainCRTStartup` in lib.rs).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-cdylib-link-arg=-nostartfiles");
        println!("cargo:rustc-cdylib-link-arg=-Wl,--enable-stdcall-fixup");
    }
    // Version information: a named, versioned library (antivirus heuristics
    // distrust anonymous ones).
    rea_launcher_build::Resources::new(
        "CMR Launcher hooks",
        "Colin McRae Rally fixes (CMR Launcher)",
    )
    .company("AkitaOnRails")
    .file_name("cmrhooks.dll")
    .dll()
    .embed()
    .unwrap();
}
