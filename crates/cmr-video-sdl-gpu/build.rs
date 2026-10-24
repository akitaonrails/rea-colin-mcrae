use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=shaders/indexed.vert.glsl");
    println!("cargo:rerun-if-changed=shaders/indexed.frag.glsl");
    println!("cargo:rerun-if-changed=shaders/c3d.vert.glsl");
    println!("cargo:rerun-if-changed=shaders/c3d.frag.glsl");
    println!("cargo:rerun-if-env-changed=CMR_GLSLANG_VALIDATOR");

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR"));
    // This is an explicit external build boundary, not a reproducible pinned
    // toolchain: callers may select their installed compiler, and no generated
    // shader binary is checked into the repository.
    let compiler = env::var_os("CMR_GLSLANG_VALIDATOR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("glslangValidator"));
    let vertex = compile(
        &compiler,
        Path::new("shaders/indexed.vert.glsl"),
        &output.join("indexed.vert.spv"),
        "vert",
    );
    let fragment = compile(
        &compiler,
        Path::new("shaders/indexed.frag.glsl"),
        &output.join("indexed.frag.spv"),
        "frag",
    );
    let c3d_vertex = compile(
        &compiler,
        Path::new("shaders/c3d.vert.glsl"),
        &output.join("c3d.vert.spv"),
        "vert",
    );
    let c3d_fragment = compile(
        &compiler,
        Path::new("shaders/c3d.frag.glsl"),
        &output.join("c3d.frag.spv"),
        "frag",
    );
    let available = vertex && fragment && c3d_vertex && c3d_fragment;
    if !available {
        fs::write(output.join("indexed.vert.spv"), []).expect("write empty vertex marker");
        fs::write(output.join("indexed.frag.spv"), []).expect("write empty fragment marker");
        fs::write(output.join("c3d.vert.spv"), []).expect("write empty C3D vertex marker");
        fs::write(output.join("c3d.frag.spv"), []).expect("write empty C3D fragment marker");
        println!(
            "cargo:warning=SDL_GPU SPIR-V shaders unavailable; set CMR_GLSLANG_VALIDATOR to an external glslangValidator to enable hardware conformance (the toolchain is currently unpinned)"
        );
    }
    println!(
        "cargo:rustc-env=CMR_SDL_GPU_SPIRV_AVAILABLE={}",
        if available { "1" } else { "0" }
    );
}

fn compile(compiler: &Path, source: &Path, output: &Path, stage: &str) -> bool {
    Command::new(compiler)
        .args(["-V", "--target-env", "vulkan1.0", "-S", stage, "-o"])
        .arg(output)
        .arg(source)
        .status()
        .is_ok_and(|status| status.success())
}
