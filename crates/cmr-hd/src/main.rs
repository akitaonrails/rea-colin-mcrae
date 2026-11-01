//! `cmr-hd`: extracts the 2D images (menus, loading screens, HUD) of the
//! player's own game folder and upscales them with Real-ESRGAN, all on the
//! player's machine. Nothing it produces is ever committed or shipped: the
//! images come from the game. Design and findings: docs/UPSCALING.md.
//!
//!   cmr-hd list <game folder>
//!   cmr-hd extract <game folder> <out folder> [--all]
//!   cmr-hd upscale <extracted folder> <hd folder> [--esrgan <realesrgan-ncnn-vulkan>] [--only <path prefix>]
//!   cmr-hd pack <hd folder> <pack folder>
//!   cmr-hd import <extracted folder> <redrawn folder> <hd folder>
//!
//! `upscale` writes two sets: `x4/` (Real-ESRGAN's 4x, for 1440p and up)
//! and `x2/` (the 4x made half as large, for 1080p).

mod asset;
mod atlas;
mod classes;
mod depixel;
/// The runtime names of HD bitmaps: the hook library's own source file, so
/// the tool and the game side can't disagree (compiled here, not linked:
/// on Windows the hook library is built without `std`).
#[path = "../../cmr-hooks/src/hd.rs"]
mod hd;
mod key;
mod lock;
mod pack;
mod palette;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use classes::Class;
use image::RgbaImage;
use sha2::{Digest, Sha256};

const USAGE: &str = "usage:
  cmr-hd list <game folder>
  cmr-hd extract <game folder> <out folder> [--all]
  cmr-hd upscale <extracted folder> <hd folder> [--esrgan <realesrgan-ncnn-vulkan>] [--only <path prefix>]
  cmr-hd pack <hd folder> <pack folder>
  cmr-hd import <extracted folder> <redrawn folder> <hd folder>

extract writes each image as PNG (keyed sprites with transparency) and a
manifest.tsv; upscale redraws flat art with smooth outlines and runs
Real-ESRGAN 4x on the rest by class (without --esrgan, flat art only),
writing x4/ (1440p and up) and x2/ (1080p, the 4x made half as large).
See docs/UPSCALING.md. pack stores an HD set compactly for the launcher
to install. import adds images redrawn elsewhere (at 4x, as <path>.png,
with transparency for keyed images) to an HD set, replacing its own.";

/// The upscale factor of every Real-ESRGAN model used here.
const SCALE: u32 = 4;
const MANIFEST: &str = "manifest.tsv";
/// The largest side of a sheet, in source pixels (4 Mpixels, 64 after 4x).
const SHEET: u32 = 2048;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("list") if args.len() == 2 => list(Path::new(&args[1])),
        Some("extract") if args.len() >= 3 => extract(
            Path::new(&args[1]),
            Path::new(&args[2]),
            args.iter().any(|a| a == "--all"),
        ),
        Some("pack") if args.len() == 3 => {
            let hd = Path::new(&args[1]);
            keyed_names(hd).and_then(|keyed| pack::pack(hd, Path::new(&args[2]), &keyed))
        }
        Some("import") if args.len() == 4 => import(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
        ),
        Some("upscale") if args.len() >= 3 => {
            let flag = |name: &str| {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|i| args.get(i + 1))
                    .cloned()
            };
            upscale(
                Path::new(&args[1]),
                Path::new(&args[2]),
                flag("--esrgan").as_deref().map(Path::new),
                flag("--only").as_deref(),
            )
        }
        _ => Err(USAGE.to_string()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Whether each runtime name is a keyed image, from an HD set's manifest.
fn keyed_names(hd: &Path) -> Result<BTreeMap<String, bool>, String> {
    let manifest = fs::read_to_string(hd.join(MANIFEST))
        .map_err(|e| format!("no {MANIFEST} in {}: {e}", hd.display()))?;
    Ok(manifest
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some((f.get(5)?.to_string(), Class::from_name(f.get(1)?).keyed()))
        })
        .collect())
}

/// Every `.bmp` under `root`, as forward-slash paths relative to it, and
/// the bitmaps inside each stage's horizon (`<path>.hzn/<n>.bmp`, see
/// `HORIZON`).
fn bitmaps(root: &Path) -> Result<Vec<String>, String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let is = |ext: &str| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(ext))
            };
            if path.is_dir() {
                walk(&path, root, out)?;
            } else if is("bmp") || is("hzn") {
                let rel = path.strip_prefix(root).expect("under root");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files).map_err(|e| format!("cannot read {}: {e}", root.display()))?;
    let mut out = Vec::new();
    for rel in files {
        if rel.to_ascii_lowercase().ends_with(".hzn") {
            let stored = fs::read(root.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
            let model = asset::unwrap(&stored).map_err(|e| format!("{rel}: {e}"))?;
            out.extend(
                (0..asset::embedded_bitmaps(&model).len()).map(|n| format!("{rel}/{n}.bmp")),
            );
        } else {
            out.push(rel);
        }
    }
    out.sort();
    Ok(out)
}

/// Marks a bitmap inside a horizon file: `<path>.hzn/<n>.bmp`.
const HORIZON: &str = ".hzn/";

/// A horizon bitmap's file and index, from its path.
fn in_horizon(rel: &str) -> Option<(String, usize)> {
    let at = rel.to_ascii_lowercase().find(HORIZON)?;
    let n = rel[at + HORIZON.len()..]
        .strip_suffix(".bmp")?
        .parse()
        .ok()?;
    Some((rel[..at + HORIZON.len() - 1].to_string(), n))
}

/// A game image.
struct Decoded {
    /// Its pixels.
    img: image::RgbaImage,
    /// The SHA-256 of the game file.
    digest: String,
    /// The name its HD version has at run time (`hd::name`).
    name: String,
    /// The BMP the game loads (its palette makes the 8-bit HD version).
    bmp: Vec<u8>,
}

fn decode(game: &Path, rel: &str) -> Result<Decoded, String> {
    let horizon = in_horizon(rel);
    let file = horizon.as_ref().map_or(rel, |(file, _)| file);
    let stored = fs::read(game.join(file)).map_err(|e| format!("{rel}: {e}"))?;
    let digest = Sha256::digest(&stored)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let mut bmp = asset::unwrap(&stored).map_err(|e| format!("{rel}: {e}"))?;
    if let Some((_, n)) = horizon {
        bmp = asset::embedded_bitmaps(&bmp)
            .get(n)
            .ok_or_else(|| format!("{rel}: no bitmap {n} in the horizon"))?
            .to_vec();
    }
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
        .map_err(|e| format!("{rel}: {e}"))?;
    // The same bytes the hook library hashes: the header's bfSize.
    let header: [u8; 6] = bmp
        .get(..6)
        .and_then(|h| h.try_into().ok())
        .ok_or_else(|| format!("{rel}: too short for a BMP"))?;
    let len = hd::bmp_len(&header)
        .filter(|&n| n <= bmp.len())
        .ok_or_else(|| format!("{rel}: its BMP header has no usable size"))?;
    let name = String::from_utf8_lossy(&hd::name(&bmp[..len])).into_owned();
    Ok(Decoded {
        img: img.to_rgba8(),
        digest,
        name,
        bmp,
    })
}

fn list(game: &Path) -> Result<(), String> {
    let mut by_class: BTreeMap<&str, (u32, u64)> = BTreeMap::new();
    for rel in bitmaps(game)? {
        let class = classes::classify(&rel);
        let pixels = decode(game, &rel)
            .map(|d| u64::from(d.img.width()) * u64::from(d.img.height()))
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
    let mut manifest =
        String::from("# path\tclass\tsha256 of the game file\twidth\theight\truntime name\n");
    let (mut done, mut failed) = (0, 0);
    for rel in bitmaps(game)? {
        let class = classes::classify(&rel);
        if class == Class::Skip && !all {
            continue;
        }
        match decode(game, &rel) {
            Ok(Decoded {
                mut img,
                digest,
                name,
                bmp,
            }) => {
                if class.keyed() {
                    key::to_alpha(&mut img);
                }
                let path = png_path(out, &rel);
                fs::create_dir_all(path.parent().expect("a parent"))
                    .and_then(|()| img.save(&path).map_err(std::io::Error::other))
                    .and_then(|()| fs::write(out.join(format!("{rel}.orig.bmp")), &bmp))
                    .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
                manifest.push_str(&format!(
                    "{rel}\t{}\t{digest}\t{}\t{}\t{name}\n",
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
    /// The runtime name of its HD bitmaps.
    name: &'a str,
    /// Its pixels (keyed images with transparency).
    img: RgbaImage,
    rest: String,
}

/// How an image is enlarged: flat art (the HUD; logos, flags and icons of
/// few colours) is redrawn with smooth outlines (`depixel`), as if drawn
/// with vector lines; the rest by Real-ESRGAN with this model.
fn method(class: Class, img: &RgbaImage) -> Option<&'static str> {
    let colours = img
        .pixels()
        .map(|p| p.0)
        .collect::<std::collections::HashSet<_>>()
        .len();
    if class.flat(colours) {
        Some(DEPIXEL)
    } else {
        class.model()
    }
}

const DEPIXEL: &str = "depixel";

/// The largest side of an HD twin the game loads, in pixels.
const MAX_TWIN: u32 = 8192;

/// Writes the BMP the game loads in place of `original`: for a texture
/// (the HUD) with an 8-bit original, 8-bit in the original's palette (the
/// game keeps those as indices); else 24-bit (2D surfaces are converted to
/// the screen's 16 bits as they load, so the smooth edges keep their
/// in-between shades).
fn write_runtime(
    img: &image::RgbaImage,
    original: &[u8],
    class: Class,
    path: &Path,
) -> Result<(), String> {
    let mut file = fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = image::codecs::bmp::BmpEncoder::new(&mut file);
    let palette = palette::of_bmp(original).filter(|_| class.texture());
    let result = match palette {
        Some(colours) => {
            let indices: Vec<u8> = img
                .pixels()
                .map(|p| palette::index(&colours, p.0, class.keyed()))
                .collect();
            encoder.encode_with_palette(
                &indices,
                img.width(),
                img.height(),
                image::ExtendedColorType::L8,
                Some(&colours),
            )
        }
        None => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
            encoder.encode(
                &rgb,
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
        }
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}

/// Runs Real-ESRGAN 4x on one image file and reads the result.
fn run_esrgan(
    esrgan: &Path,
    model: &str,
    input: &Path,
    output: &Path,
) -> Result<RgbaImage, String> {
    let mut cmd = Command::new(esrgan);
    cmd.arg("-i").arg(input).arg("-o").arg(output);
    cmd.args(["-n", model, "-s", &SCALE.to_string(), "-f", "png"]);
    if let Some(m) = esrgan
        .parent()
        .map(|d| d.join("models"))
        .filter(|m| m.is_dir())
    {
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
    image::open(output)
        .map(|i| i.to_rgba8())
        .map_err(|e| format!("{}: {e}", output.display()))
}

/// The 4x and 2x versions of a flat image, redrawn with smooth outlines.
fn redraw(job: &Job) -> (RgbaImage, RgbaImage) {
    (depixel_edges(&job.img, 4), depixel_edges(&job.img, 2))
}

/// A texture's HD versions (the HUD: 8-bit at run time) held to the
/// original's own colours, whatever made them: each pixel takes one of the
/// colours the original has around the spot. Colours stay the artist's,
/// and transparency stays where the original has it; an image model's
/// near-black (a pace-note arrow) would otherwise become the colour key.
fn held(
    class: Class,
    original: &RgbaImage,
    (x4, x2): (RgbaImage, RgbaImage),
) -> (RgbaImage, RgbaImage) {
    if !class.texture() {
        return (x4, x2);
    }
    let (w, h) = original.dimensions();
    // Inside: the original pixel and its eight neighbours are all opaque.
    let inside = |x: u32, y: u32| {
        (x.saturating_sub(1)..=(x + 1).min(w - 1)).all(|nx| {
            (y.saturating_sub(1)..=(y + 1).min(h - 1)).all(|ny| original.get_pixel(nx, ny).0[3] > 0)
        })
    };
    let lock = |mut up: RgbaImage| {
        let scale = up.width() / w.max(1);
        if scale == 0 || up.width() != w * scale {
            return up; // not a whole multiple of the original
        }
        for (x, y, p) in up.enumerate_pixels_mut() {
            if inside(x / scale, y / scale) {
                p.0[3] = 255;
            }
        }
        lock::exact(original, &up, scale)
    };
    (lock(x4), lock(x2))
}

/// Pixels a flat image is continued by on every side before it is redrawn.
const EDGE: u32 = 4;

/// `depixel::scale` with the image's edge pixels continued past its border
/// first, then cut back: the game joins sprites edge to edge and stretches
/// thin tiles (the HUD frame's corners meet its bars), so a line that runs
/// off the edge keeps its course and width there instead of ending in a
/// curve of its own.
fn depixel_edges(img: &RgbaImage, scale: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let wide = RgbaImage::from_fn(w + 2 * EDGE, h + 2 * EDGE, |x, y| {
        *img.get_pixel(
            x.saturating_sub(EDGE).min(w - 1),
            y.saturating_sub(EDGE).min(h - 1),
        )
    });
    let up = depixel::scale(&wide, scale);
    image::imageops::crop_imm(&up, EDGE * scale, EDGE * scale, w * scale, h * scale).to_image()
}

/// The 4x and 2x versions from Real-ESRGAN's 4x: the 2x made half as large
/// (sharper than enlarging the original twice), both held to the
/// original's colours.
fn from_esrgan(job: &Job, x4: RgbaImage) -> (RgbaImage, RgbaImage) {
    let x2 = image::imageops::resize(
        &x4,
        x4.width() / 2,
        x4.height() / 2,
        image::imageops::FilterType::Lanczos3,
    );
    (
        lock::back_project(&job.img, &x4, 4),
        lock::back_project(&job.img, &x2, 2),
    )
}

/// Saves an image's 4x and 2x versions as PNG under `x4/` and `x2/`, plus
/// the BMP the game loads (none when the twin would be wider or taller
/// than `MAX_TWIN`, like the menu's panorama enlarged 4x).
fn save_sets(job: &Job, sets: (RgbaImage, RgbaImage), src: &Path, hd: &Path) -> Result<(), String> {
    let (mut x4, mut x2) = held(job.class, &job.img, sets);
    if job.class.keyed() {
        key::to_key(&mut x4);
        key::to_key(&mut x2);
    }
    let runtime = job.class.runtime() && x4.width() <= MAX_TWIN && x4.height() <= MAX_TWIN;
    for (set, img) in [("x4", &x4), ("x2", &x2)] {
        let output = png_path(&hd.join(set), job.rel);
        fs::create_dir_all(output.parent().expect("a parent")).map_err(|e| e.to_string())?;
        img.save(&output).map_err(|e| e.to_string())?;
        if runtime {
            let original = fs::read(src.join(format!("{}.orig.bmp", job.rel)))
                .map_err(|e| format!("{}: {e}", job.rel))?;
            write_runtime(
                img,
                &original,
                job.class,
                &hd.join(set).join(format!("{}.bmp", job.name)),
            )?;
        }
    }
    Ok(())
}

/// An HD set from images redrawn elsewhere (`redrawn/<path>.png`, at 4x;
/// wide images may be smaller): each becomes the 4x set, half of it the 2x
/// set, with the same keying and runtime BMPs as `upscale`.
fn import(src: &Path, redrawn: &Path, hd: &Path) -> Result<(), String> {
    let manifest = fs::read_to_string(src.join(MANIFEST))
        .map_err(|e| format!("no {MANIFEST} in {}: {e}", src.display()))?;
    let mut rows = read_rows(hd);
    let mut imported = BTreeMap::new();
    let mut done = 0;
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [rel, class, digest, w, h, name] = f[..] else {
            continue;
        };
        let Ok(up) = image::open(png_path(redrawn, rel)) else {
            continue;
        };
        let up = up.to_rgba8();
        let class = Class::from_name(class);
        let job = Job {
            rel,
            class,
            name,
            img: image::open(png_path(src, rel))
                .map_err(|e| format!("{rel}: {e}"))?
                .to_rgba8(),
            rest: format!("{}\t{digest}\t{w}\t{h}\t{name}\tredrawn", class.name()),
        };
        let x2 = image::imageops::resize(
            &up,
            (up.width() / 2).max(1),
            (up.height() / 2).max(1),
            image::imageops::FilterType::Lanczos3,
        );
        save_sets(&job, (up, x2), src, hd)?;
        rows.insert(rel.to_string(), job.rest.clone());
        imported.insert(name, job.rest);
        done += 1;
    }
    // The same game file at other paths shares the twin just written.
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        if let (Some(rel), Some(rest)) = (f.first(), f.get(5).and_then(|n| imported.get(n))) {
            rows.insert(rel.to_string(), rest.clone());
        }
    }
    write_rows(hd, &rows)?;
    println!("{done} redrawn images imported into {}", hd.display());
    Ok(())
}

/// Upscales every image the manifest lists, by method (`method`).
fn upscale(src: &Path, hd: &Path, esrgan: Option<&Path>, only: Option<&str>) -> Result<(), String> {
    let manifest = fs::read_to_string(src.join(MANIFEST)).map_err(|e| {
        format!(
            "no {MANIFEST} in {} (run extract first): {e}",
            src.display()
        )
    })?;
    let mut jobs: BTreeMap<&str, Vec<Job>> = BTreeMap::new();
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let [rel, class, digest, w, h, name] = f[..] else {
            continue;
        };
        let class = Class::from_name(class);
        if class.model().is_none() || only.is_some_and(|p| !rel.starts_with(p)) {
            continue;
        }
        let img = image::open(png_path(src, rel))
            .map_err(|e| format!("{rel}: {e}"))?
            .to_rgba8();
        let Some(model) = method(class, &img).filter(|&m| m == DEPIXEL || esrgan.is_some()) else {
            continue;
        };
        let rest = format!("{}\t{digest}\t{w}\t{h}\t{name}\t{model}", class.name());
        jobs.entry(model).or_default().push(Job {
            rel,
            class,
            name,
            img,
            rest,
        });
    }
    let mut rows = read_rows(hd);
    let mut done = 0;
    for (model, jobs) in &jobs {
        if *model == DEPIXEL {
            println!("{model}: {} images", jobs.len());
            for job in jobs {
                save_sets(job, redraw(job), src, hd)?;
            }
        } else if let Some(esrgan) = esrgan {
            upscale_sheets(model, jobs, src, hd, esrgan)?;
        }
        for job in jobs {
            rows.insert(job.rel.to_string(), job.rest.clone());
            done += 1;
        }
    }
    write_rows(hd, &rows)?;
    println!(
        "{done} images upscaled into {}/x4 and {}/x2",
        hd.display(),
        hd.display()
    );
    Ok(())
}

/// An HD set's manifest rows (path to the rest of the row), empty if it
/// has none yet: upscale and import add to a set, replacing their paths.
fn read_rows(hd: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(hd.join(MANIFEST))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(rel, rest)| (rel.to_string(), rest.to_string()))
        .collect()
}

fn write_rows(hd: &Path, rows: &BTreeMap<String, String>) -> Result<(), String> {
    let mut out = String::from(
        "# path\tclass\tsha256 of the game file\twidth\theight\truntime name\tmodel (x4/ and x2/ hold the 4x and 2x sets)\n",
    );
    for (rel, rest) in rows {
        out.push_str(&format!("{rel}\t{rest}\n"));
    }
    fs::create_dir_all(hd).map_err(|e| e.to_string())?;
    fs::write(hd.join(MANIFEST), out).map_err(|e| e.to_string())
}

/// Real-ESRGAN over one model's images, packed into sheets (`atlas`): one
/// upscaler run per sheet.
fn upscale_sheets(
    model: &str,
    jobs: &[Job],
    src: &Path,
    hd: &Path,
    esrgan: &Path,
) -> Result<(), String> {
    {
        let batch = hd.join(format!(".batch-{model}"));
        let _ = fs::remove_dir_all(&batch);
        fs::create_dir_all(&batch).map_err(|e| e.to_string())?;
        let sizes: Vec<_> = jobs.iter().map(|j| j.img.dimensions()).collect();
        let sheets = atlas::pack(&sizes, SHEET);
        println!("{model}: {} images on {} sheets", jobs.len(), sheets.len());
        for (n, sheet) in sheets.iter().enumerate() {
            let mut canvas = RgbaImage::new(sheet.width, sheet.height);
            for p in &sheet.places {
                atlas::draw(&mut canvas, &jobs[p.index].img, p.x, p.y);
            }
            let (input, output) = (
                batch.join(format!("{n}.png")),
                batch.join(format!("{n}.x4.png")),
            );
            canvas.save(&input).map_err(|e| e.to_string())?;
            let up = run_esrgan(esrgan, model, &input, &output)?;
            if up.dimensions() != (sheet.width * SCALE, sheet.height * SCALE) {
                return Err(format!(
                    "{model}: the upscaler returned a sheet of the wrong size"
                ));
            }
            for p in &sheet.places {
                let (w, h) = sizes[p.index];
                let x4 =
                    image::imageops::crop_imm(&up, p.x * SCALE, p.y * SCALE, w * SCALE, h * SCALE)
                        .to_image();
                save_sets(&jobs[p.index], from_esrgan(&jobs[p.index], x4), src, hd)?;
            }
        }
        let _ = fs::remove_dir_all(&batch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// The HUD's vertical bar tile: grey, white, white, grey.
    fn bar() -> RgbaImage {
        RgbaImage::from_fn(4, 4, |x, _| match x {
            1 | 2 => Rgba([230, 230, 230, 255]),
            _ => Rgba([90, 90, 90, 255]),
        })
    }

    #[test]
    fn a_stretched_tile_stays_straight_to_its_edges() {
        let up = depixel_edges(&bar(), 4);
        assert_eq!(up.dimensions(), (16, 16));
        for y in 1..16 {
            for x in 0..16 {
                assert_eq!(up.get_pixel(x, y), up.get_pixel(x, 0), "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_line_leaving_the_edge_keeps_its_width_there() {
        // The bar's top half, then it turns right: at the bottom edge it
        // must still be the bar (as the tile it joins continues it).
        let img = RgbaImage::from_fn(8, 8, |x, y| {
            let core = (x == 1 || x == 2) && y >= 4 || (y == 1 || y == 2) && x >= 1;
            let edge = (x == 0 || x == 3) && y >= 3 || y == 0 || y == 3;
            if core {
                Rgba([230, 230, 230, 255])
            } else if edge {
                Rgba([90, 90, 90, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let up = depixel_edges(&img, 4);
        let tile = depixel_edges(&bar(), 4);
        for x in 0..16 {
            assert_eq!(up.get_pixel(x, 31), tile.get_pixel(x, 0), "x {x}");
        }
    }

    #[test]
    fn sets_keep_their_rows_and_new_ones_replace_them() {
        let hd = std::env::temp_dir().join(format!("cmr-hd-rows-{}", std::process::id()));
        let _ = fs::remove_dir_all(&hd);
        let mut rows = read_rows(&hd);
        assert!(rows.is_empty());
        rows.insert("a.bmp".into(), "hud\tx\t1\t1\tn\tdepixel".into());
        rows.insert("b.bmp".into(), "picture\ty\t1\t1\tm\tdepixel".into());
        write_rows(&hd, &rows).unwrap();
        let mut again = read_rows(&hd);
        assert_eq!(again, rows);
        again.insert("b.bmp".into(), "picture\ty\t1\t1\tm\tredrawn".into());
        write_rows(&hd, &again).unwrap();
        let last = read_rows(&hd);
        assert_eq!(last["a.bmp"], rows["a.bmp"]);
        assert!(last["b.bmp"].ends_with("redrawn"));
        let _ = fs::remove_dir_all(&hd);
    }

    #[test]
    fn redrawn_textures_keep_the_originals_colours_and_transparency() {
        // A pace-note sign: transparent corner ring, yellow fill, a dark
        // grey arrow in the middle.
        let original = RgbaImage::from_fn(8, 8, |x, y| {
            if x == 0 || y == 0 || x == 7 || y == 7 {
                Rgba([0, 0, 0, 0])
            } else if (3..5).contains(&x) && (2..6).contains(&y) {
                Rgba([24, 24, 24, 255])
            } else {
                Rgba([250, 200, 0, 255])
            }
        });
        // An image model's redraw: the fill drifted to ochre, the arrow
        // came back pure black (keyed as transparent).
        let redraw = |s: u32| {
            RgbaImage::from_fn(8 * s, 8 * s, |x, y| {
                let o = original.get_pixel(x / s, y / s).0;
                match o {
                    [0, 0, 0, 0] => Rgba([0, 0, 0, 0]),
                    [24, ..] => Rgba([0, 0, 0, 0]),
                    _ => Rgba([200, 150, 20, 255]),
                }
            })
        };
        let (x4, x2) = held(Class::Hud, &original, (redraw(4), redraw(2)));
        for (img, s) in [(&x4, 4), (&x2, 2)] {
            for y in 0..8 * s {
                for x in 0..8 * s {
                    let want = original.get_pixel(x / s, y / s);
                    // Away from the edges between colours, the original's.
                    let inner = |v: u32| !v.is_multiple_of(s) && v % s != s - 1;
                    if inner(x) && inner(y) {
                        assert_eq!(img.get_pixel(x, y), want, "({x}, {y}) at {s}x");
                    }
                }
            }
        }
        // Pictures are left alone.
        let (p, _) = held(Class::Picture, &original, (redraw(4), redraw(2)));
        assert_eq!(p, redraw(4));
    }

    #[test]
    fn horizon_paths() {
        assert_eq!(
            in_horizon("Game/NZ/1/nz1.hzn/2.bmp"),
            Some(("Game/NZ/1/nz1.hzn".to_string(), 2))
        );
        assert_eq!(
            in_horizon("Game/Au/1/AU1.HZN/0.bmp"),
            Some(("Game/Au/1/AU1.HZN".to_string(), 0))
        );
        assert_eq!(in_horizon("Game/Images/osdnew/ARRWS.BMP"), None);
    }
}
