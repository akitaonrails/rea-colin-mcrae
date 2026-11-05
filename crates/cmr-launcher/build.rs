//! CMRLauncher.exe's version information, icon and manifest
//! (rea-launcher-build): a named, versioned program with the launcher's
//! icon, which antivirus heuristics trust more than an anonymous one.
fn main() {
    rea_launcher_build::Resources::new("CMR Launcher", "Colin McRae Rally launcher")
        .company("AkitaOnRails")
        .file_name("CMRLauncher.exe")
        .icon("../../packaging/appimage/cmr-launcher.png")
        .embed()
        .unwrap();
}
