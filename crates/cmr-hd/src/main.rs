//! `cmr-hd`: extracts the 2D images (menus, loading screens, HUD) of the
//! player's own game folder and upscales them with Real-ESRGAN, all on the
//! player's machine. Nothing it produces is ever committed or shipped: the
//! images come from the game. Design and findings: docs/UPSCALING.md.
//!
//!   cmr-hd list <game folder>
//!   cmr-hd extract <game folder> <out folder> [--all]
//!   cmr-hd upscale <extracted folder> <hd folder> --esrgan <realesrgan-ncnn-vulkan> [--only <path prefix>]
//!
//! `upscale` writes two sets: `x4/` (Real-ESRGAN's 4x, for 1440p and up)
//! and `x2/` (the 4x made half as large, for 1080p).

mod asset;
mod classes;
mod key;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use classes::Class;
use sha2::{Digest, Sha256};

const USAGE: &str = "usage:
  cmr-hd list <game folder>
  cmr-hd extract <game folder> <out folder> [--all]
  cmr-hd upscale <extracted folder> <hd folder> --esrgan <realesrgan-ncnn-vulkan> [--only <path prefix>]

extract writes each image as PNG (keyed sprites with transparency) and a
manifest.tsv; upscale runs Real-ESRGAN 4x on them by class and writes
x4/ (1440p and up) and x2/ (1080p, the 4x made half as large). See
docs/UPSCALING.md. The outputs come from your game: keep them to yourself.";

/// The upscale factor of every Real-ESRGAN model used here.
const SCALE: u32 = 4;
const MANIFEST: &str = "manifest.tsv";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("list") if args.len() == 2 => list(Path::new(&args[1])),
        Some("extract") if args.len() >= 3 => extract(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.iter().any(|a| a == "--all"),
        ),
        Some("upscale") if args.len() >= 3 => {
            let flag = |name: &str| {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
            };
            match flag("--esrgan") {
                Some(esrgan) => upscale(
                    Path::new(&args[1]),
                    Path::new(&args[2]),
                    Path::new(&esrgan),
                    flag("--only").as_deref(),
                ),
                None => Err(format!("upscale needs --esrgan\n\n{USAGE}")),
            }
        }
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Every `.bmp` under `root`, as forward-slash paths relative to it.
fn bitmaps(root: &Path) -> Result<Vec<String>, String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, root, out)?;
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("bmp"))
            {
                let rel = path.strip_prefix(root).expect("under root");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
    out.sort();
    Ok(out)
}

fn decode(game: &Path, rel: &str) -> Result<(image::RgbaImage, String), String> {
    let stored = fs::read(game.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
    let digest = Sha256::digest(&stored)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let bmp = asset::unwrap(&stored).map_err(|e| format!("{rel}: {e}"))?;
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
        .map_err(|e| format!("{rel}: {e}"))?;
    Ok((img.to_rgba8(), digest))
}

fn list(game: &Path) -> Result<(), String> {
    let mut by_class: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    for rel in bitmaps(game)? {
        let class = classes::classify(&rel);
        let pixels = decode(game, &rel)
            .map(|(img, _)| u64::from(img.width()) * u64::from(img.height()))
            .unwrap_or(0);
        let e = by_class.entry(class.name()).or_default();
        e.0 += 1;
        e.1 += pixels;
    }
    for (class, (n, pixels)) in by_class {
        println!(
            "{class:10} {n:5} images, {:.1} Mpixels",
            pixels as f64 / 1e6
        );
    }
    Ok(())
}

fn png_path(out: &Path, rel: &str) -> PathBuf {
    out.join(format!("{rel}.png"))
}

fn extract(game: &Path, out: &Path, all: bool) -> Result<(), String> {
    let mut manifest = String::from("# path\tclass\tsha256 of the game file\twidth\theight\n");
    let (mut done, mut failed) = (0, 0);
    for rel in bitmaps(game)? {
        let class = classes::classify(&rel);
        if class == Class::Skip && !all {
            continue;
        }
        match decode(game, &rel) {
            Ok((mut img, digest)) => {
                if class.keyed() {
                    key::to_alpha(&mut img);
                }
                let path = png_path(out, &rel);
                fs::create_dir_all(path.parent().expect("a parent"))
                    .and_then(|()| img.save(&path).map_err(std::io::Error::other))
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                manifest.push_str(&format!(
                    "{rel}\t{}\t{digest}\t{}\t{}\n",
                    class.name(),
                    img.width(),
                    img.height()
                ));
                done += 1;
            }
            Err(e) => {
                eprintln!("skipped: {e}");
                failed += 1;
            }
        }
    }
    fs::write(out.join(MANIFEST), manifest).map_err(|e| e.to_string())?;
    println!(
        "{done} images extracted to {} ({failed} skipped)",
        out.display()
    );
    Ok(())
}

/// One manifest line to upscale.
struct Job<'a> {
    rel: &'a str,
    class: Class,
    rest: String,
}

/// Upscales by model in batches: Real-ESRGAN loads its model once per run
/// (seconds), so each model's images go through one run over a folder of
/// numbered copies, then move to their own paths.
fn upscale(src: &Path, hd: &Path, esrgan: &Path, only: Option<&str>) -> Result<(), String> {
    let manifest = fs::read_to_string(src.join(MANIFEST)).map_err(|e| {
        format!(
            "no {MANIFEST} in {} (run extract first): {e}",
            src.display()
        )
    })?;
    let mut jobs: BTreeMap<&str, Vec<Job>> = BTreeMap::new();
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [rel, class, digest, w, h] = f[..] else {
            continue;
        };
        let class = Class::from_name(class);
        let Some(model) = class.model() else { continue };
        if only.is_some_and(|p| !rel.starts_with(p)) {
            continue;
        }
        let rest = format!("{}\t{digest}\t{w}\t{h}\t{model}", class.name());
        jobs.entry(model)
            .or_default()
            .push(Job { rel, class, rest });
    }
    let models = esrgan.parent().map(|d| d.join("models"));
    let mut out = String::from(
        "# path\tclass\tsha256 of the game file\twidth\theight\tmodel (x4/ and x2/ hold the 4x and 2x sets)\n",
    );
    let mut done = 0;
    for (model, jobs) in &jobs {
        let batch = hd.join(format!(".batch-{model}"));
        let (bin, bout) = (batch.join("in"), batch.join("out"));
        let _ = fs::remove_dir_all(&batch);
        fs::create_dir_all(&bin)
            .and_then(|()| fs::create_dir_all(&bout))
            .map_err(|e| e.to_string())?;
        for (i, job) in jobs.iter().enumerate() {
            fs::copy(png_path(src, job.rel), bin.join(format!("{i:05}.png")))
                .map_err(|e| format!("{}: {e}", job.rel))?;
        }
        println!("{model}: {} images", jobs.len());
        let mut cmd = Command::new(esrgan);
        cmd.arg("-i").arg(&bin).arg("-o").arg(&bout);
        cmd.args(["-n", model, "-s", &SCALE.to_string(), "-f", "png"]);
        if let Some(m) = models.as_ref().filter(|m| m.is_dir()) {
            cmd.arg("-m").arg(m);
        }
        let run = cmd
            .output()
            .map_err(|e| format!("cannot run {}: {e}", esrgan.display()))?;
        if !run.status.success() {
            return Err(format!(
                "{model}: the upscaler failed: {}",
                String::from_utf8_lossy(&run.stderr)
                    .lines()
                    .last()
                    .unwrap_or("")
            ));
        }
        for (i, job) in jobs.iter().enumerate() {
            let result = bout.join(format!("{i:05}.png"));
            if !result.is_file() {
                eprintln!("skipped: {}: no output", job.rel);
                continue;
            }
            let x4 = image::open(&result)
                .map_err(|e| format!("{}: {e}", result.display()))?
                .to_rgba8();
            // The 2x set, for 1080p: the 4x result made half as large (a
            // sharper result than enlarging the original twice).
            let mut x2 = image::imageops::resize(
                &x4,
                (x4.width() / 2).max(1),
                (x4.height() / 2).max(1),
                image::imageops::FilterType::Lanczos3,
            );
            let mut x4 = x4;
            if job.class.keyed() {
                key::to_key(&mut x4);
                key::to_key(&mut x2);
            }
            for (set, img) in [("x4", &x4), ("x2", &x2)] {
                let output = png_path(&hd.join(set), job.rel);
                fs::create_dir_all(output.parent().expect("a parent"))
                    .map_err(|e| e.to_string())?;
                img.save(&output).map_err(|e| e.to_string())?;
            }
            out.push_str(&format!("{}\t{}\n", job.rel, job.rest));
            done += 1;
        }
        let _ = fs::remove_dir_all(&batch);
    }
    fs::write(hd.join(MANIFEST), out).map_err(|e| e.to_string())?;
    println!(
        "{done} images upscaled into {}/x4 and {}/x2",
        hd.display(),
        hd.display()
    );
    Ok(())
}
