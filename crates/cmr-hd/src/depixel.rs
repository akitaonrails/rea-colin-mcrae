//! Depixelizing pixel art (Kopf and Lischinski, "Depixelizing Pixel Art",
//! SIGGRAPH 2011): smooth, vector-like edges for flat pixel art (curves
//! stay curves, diagonals become straight lines) at any integer scale.
//!
//! This is a Rust port of the default path of depixel-js by Jimb Esser
//! (MIT, https://github.com/Jimbly/depixel-js, commit b85713c), itself a
//! port of Felix Kreuzer's GPU version (MIT,
//! https://github.com/falichs/Depixelizing-Pixel-Art-on-GPUs) of the paper
//! (https://johanneskopf.de/publications/pixelart/). Licences and sources
//! are listed in THIRD_PARTY.md.
//!
//! The pipeline: similarity graph of the pixel grid, valence update,
//! removal of crossing diagonals by voting, cell graph, spline control
//! point optimisation, then a Gaussian-weighted rasteriser that only mixes
//! pixels on the same side of the traced edges.
//!
//! Numerics follow the JavaScript on purpose (f64 maths, f32 storage for
//! the point arrays, the same quirks), so the output matches it. The
//! JavaScript reads outside its typed arrays in a few places and gets
//! `undefined` (0 after `| 0`, NaN in arithmetic); the `get_*` helpers
//! below reproduce that.
//!
//! Copyright (c) 2015 Felix Kreuzer
//! Copyright (c) Jimb Esser (depixel-js)
//!
//! Permission is hereby granted, free of charge, to any person obtaining a
//! copy of this software and associated documentation files (the
//! "Software"), to deal in the Software without restriction, including
//! without limitation the rights to use, copy, modify, merge, publish,
//! distribute, sublicense, and/or sell copies of the Software, and to
//! permit persons to whom the Software is furnished to do so, subject to
//! the following conditions:
//!
//! The above copyright notice and this permission notice shall be included
//! in all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
//! OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
//! MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
//! IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
//! TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
//! SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

use image::RgbaImage;

const EDGE_HORVERT: i32 = 16;
const EDGE_DIAGONAL_ULLR: i32 = 32;
const EDGE_DIAGONAL_LLUR: i32 = 64;

const HAS_NORTHERN_NEIGHBOR: i32 = 1;
const HAS_EASTERN_NEIGHBOR: i32 = 2;
const HAS_SOUTHERN_NEIGHBOR: i32 = 4;
const HAS_WESTERN_NEIGHBOR: i32 = 8;
const HAS_NORTHERN_SPLINE: i32 = 16;
const HAS_EASTERN_SPLINE: i32 = 32;
const HAS_SOUTHERN_SPLINE: i32 = 64;
const HAS_WESTERN_SPLINE: i32 = 128;
const HAS_CORRECTED_POSITION: i32 = 256;
const DONT_OPTIMIZE_N: i32 = 512;
const DONT_OPTIMIZE_E: i32 = 1024;
const DONT_OPTIMIZE_S: i32 = 2048;
const DONT_OPTIMIZE_W: i32 = 4096;
const ANY_SPLINE: i32 =
    HAS_NORTHERN_SPLINE | HAS_EASTERN_SPLINE | HAS_SOUTHERN_SPLINE | HAS_WESTERN_SPLINE;

const NORTH: i32 = 128;
const NORTHEAST: i32 = 64;
const EAST: i32 = 32;
const SOUTHEAST: i32 = 16;
const SOUTH: i32 = 8;
const SOUTHWEST: i32 = 4;
const WEST: i32 = 2;
const NORTHWEST: i32 = 1;

const STEP: f64 = 0.2;
const GAUSS_MULTIPLIER: f64 = 2.5;
const POSITIONAL_ENERGY_SCALING: f64 = 2.5;
const LIMIT_SEARCH_ITERATIONS: u32 = 20;
const R: f64 = 0.61803399;
const C: f64 = 1.0 - R;
const TOL: f64 = 0.0001;
const BRACKET_SEARCH_A: f64 = 0.1;
const BRACKET_SEARCH_B: f64 = -0.1;
const GOLD: f64 = 1.618034;
const GLIMIT: f64 = 10.0;
const TINY: f64 = 0.000000001;
const ONE_O_255: f64 = 1.0 / 255.0;

const THRESHOLD_A: f64 = 32.0 / 255.0;
const THRESHOLD_Y: f64 = 48.0 / 255.0;
const THRESHOLD_U: f64 = 7.0 / 255.0;
const THRESHOLD_V: f64 = 6.0 / 255.0;
const THRESHOLD_CONTOUR: f64 = 100.0 / 255.0;
const THRESHOLD_CONTOUR_SQ: f64 = THRESHOLD_CONTOUR * THRESHOLD_CONTOUR;

type P2 = [f64; 2];

// JavaScript Math helpers, so edge cases behave the same.

/// `Math.round`: halves round towards +infinity.
fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 { f + 1.0 } else { f }
}

/// `Math.sign`: 0 and NaN stay as they are.
fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// `Math.max` for two values: NaN wins.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// `Math.hypot` for two values, with V8's scaled, compensated summation.
fn js_hypot(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let (a, b) = (a.abs(), b.abs());
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for v in [a, b] {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    f64::sqrt(sum) * max
}

/// The source image, RGBA bytes, with clamped reads.
struct Src<'a> {
    data: &'a [u8],
    w: i32,
    h: i32,
}

impl Src<'_> {
    fn px8(&self, x: i32, y: i32) -> [u8; 4] {
        let cx = x.clamp(0, self.w - 1);
        let cy = y.clamp(0, self.h - 1);
        let i = (cy as usize * self.w as usize + cx as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }

    fn px(&self, x: i32, y: i32) -> [f64; 4] {
        let p = self.px8(x, y);
        [
            p[0] as f64 * ONE_O_255,
            p[1] as f64 * ONE_O_255,
            p[2] as f64 * ONE_O_255,
            p[3] as f64 * ONE_O_255,
        ]
    }
}

fn yuv(c: &[f64; 4]) -> (f64, f64, f64) {
    let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    (y, 0.493 * (c[2] - y), 0.877 * (c[0] - y))
}

fn is_similar(a: &[f64; 4], b: &[f64; 4], threshold: f64) -> bool {
    let (ya, ua, va) = yuv(a);
    let (yb, ub, vb) = yuv(b);
    if (a[3] - b[3]).abs() <= THRESHOLD_A * threshold {
        if a[3] + b[3] <= THRESHOLD_A * threshold {
            // All fully transparent pixels count as similar, whatever their colour.
            return true;
        }
        if (ya - yb).abs() <= THRESHOLD_Y * threshold
            && (ua - ub).abs() <= THRESHOLD_U * threshold
            && (va - vb).abs() <= THRESHOLD_V * threshold
        {
            return true;
        }
    }
    false
}

fn is_contour(src: &Src, pl: (i32, i32), pr: (i32, i32)) -> bool {
    let (ya, ua, va) = yuv(&src.px(pl.0, pl.1));
    let (yb, ub, vb) = yuv(&src.px(pr.0, pr.1));
    let (dy, du, dv) = (ya - yb, ua - ub, va - vb);
    dy * dy + du * du + dv * dv > THRESHOLD_CONTOUR_SQ
}

/// The similarity graph on the (2w+1) x (2h+1) grid: `r` holds the edge
/// kind (or the valence on pixel centres), `g` the edge direction bits.
#[derive(Clone)]
struct Graph {
    r: Vec<i32>,
    g: Vec<i32>,
    w: i32,
    h: i32,
}

impl Graph {
    fn new(w: i32, h: i32) -> Graph {
        let n = (w * h) as usize;
        Graph {
            r: vec![0; n],
            g: vec![0; n],
            w,
            h,
        }
    }

    fn at(&self, x: i32, y: i32) -> Option<usize> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            None
        } else {
            Some((y * self.w + x) as usize)
        }
    }

    fn get_r(&self, x: i32, y: i32) -> i32 {
        self.at(x, y).map_or(0, |i| self.r[i])
    }

    fn get_g(&self, x: i32, y: i32) -> i32 {
        self.at(x, y).map_or(0, |i| self.g[i])
    }

    fn set(&mut self, x: i32, y: i32, r: i32, g: i32) {
        let i = (y * self.w + x) as usize;
        self.r[i] = r;
        self.g[i] = g;
    }

    fn on_border(&self, x: i32, y: i32) -> bool {
        x == 0 || x == self.w - 1 || y == 0 || y == self.h - 1
    }
}

fn build_similarity_graph(src: &Src, threshold: f64) -> Graph {
    let mut sim = Graph::new(2 * src.w + 1, 2 * src.h + 1);
    let pixel = |gx: i32, gy: i32| ((gx - 1).div_euclid(2), (gy - 1).div_euclid(2));
    let similar =
        |a: (i32, i32), b: (i32, i32)| is_similar(&src.px(a.0, a.1), &src.px(b.0, b.1), threshold);

    for y in 0..sim.h {
        for x in 0..sim.w {
            if sim.on_border(x, y) {
                continue;
            }
            match (x & 1, y & 1) {
                (0, 0) => {
                    let mut diagonal = 0;
                    if similar(pixel(x - 1, y + 1), pixel(x + 1, y - 1)) {
                        diagonal = EDGE_DIAGONAL_ULLR;
                    }
                    if similar(pixel(x - 1, y - 1), pixel(x + 1, y + 1)) {
                        diagonal |= EDGE_DIAGONAL_LLUR;
                    }
                    sim.set(x, y, diagonal, 0);
                }
                (0, 1) if similar(pixel(x - 1, y), pixel(x + 1, y)) => {
                    sim.set(x, y, EDGE_HORVERT, 0);
                }
                (1, 0) if similar(pixel(x, y - 1), pixel(x, y + 1)) => {
                    sim.set(x, y, EDGE_HORVERT, 0);
                }
                _ => {}
            }
        }
    }
    sim
}

/// Recomputes, for every pixel centre, how many edges leave it (`r`) and in
/// which directions (`g`).
fn valence_update(sim: &Graph) -> Graph {
    let mut out = Graph::new(sim.w, sim.h);
    for y in 0..sim.h {
        for x in 0..sim.w {
            if sim.on_border(x, y) {
                continue;
            }
            if (x & 1) == 1 && (y & 1) == 1 {
                let probes = [
                    (
                        sim.get_r(x - 1, y + 1) & EDGE_DIAGONAL_ULLR == EDGE_DIAGONAL_ULLR,
                        NORTHWEST,
                    ),
                    (sim.get_r(x, y + 1) > 0, NORTH),
                    (
                        sim.get_r(x + 1, y + 1) & EDGE_DIAGONAL_LLUR == EDGE_DIAGONAL_LLUR,
                        NORTHEAST,
                    ),
                    (sim.get_r(x + 1, y) > 0, EAST),
                    (
                        sim.get_r(x + 1, y - 1) & EDGE_DIAGONAL_ULLR == EDGE_DIAGONAL_ULLR,
                        SOUTHEAST,
                    ),
                    (sim.get_r(x, y - 1) > 0, SOUTH),
                    (
                        sim.get_r(x - 1, y - 1) & EDGE_DIAGONAL_LLUR == EDGE_DIAGONAL_LLUR,
                        SOUTHWEST,
                    ),
                    (sim.get_r(x - 1, y) > 0, WEST),
                ];
                let mut valence = 0;
                let mut edges = 0;
                for (present, bit) in probes {
                    if present {
                        valence += 1;
                        edges |= bit;
                    }
                }
                out.set(x, y, valence, edges);
            } else {
                let i = (y * sim.w + x) as usize;
                out.set(x, y, sim.r[i], sim.g[i]);
            }
        }
    }
    out
}

/// Connected-component bookkeeping for the "sparse pixels" heuristic: an
/// 8x8 window around a crossing, with the two diagonals as components 1
/// and 2.
struct Sparse {
    l: [i32; 64],
    size_a: i32,
    size_b: i32,
}

type Cascade = (i32, usize, usize);

fn same(bit: i32, idx: usize) -> Cascade {
    (bit, idx, idx)
}

impl Sparse {
    fn new() -> Sparse {
        let mut l = [0; 64];
        l[8 * 3 + 3] = 1;
        l[8 * 3 + 4] = 2;
        l[8 * 4 + 3] = 2;
        l[8 * 4 + 4] = 1;
        Sparse {
            l,
            size_a: 2,
            size_b: 2,
        }
    }

    fn count(&mut self, c: i32) {
        if c == 1 {
            self.size_a += 1;
        } else if c == 2 {
            self.size_b += 1;
        }
    }

    /// One cell of the window. If it has no component yet it takes the
    /// first connected neighbour's (the `(bit, check, take)` list is in
    /// priority order); then, once it has one, it hands it on to every
    /// connected, still unlabelled neighbour in `spread`.
    fn visit(&mut self, nhood: i32, cur: usize, cascade: &[Cascade], spread: &[(i32, usize)]) {
        let mut component = self.l[cur];
        if component == 0 {
            for &(bit, check, take) in cascade {
                if nhood & bit != 0 && self.l[check] != 0 {
                    component = self.l[take];
                    self.l[cur] = component;
                    self.count(component);
                    break;
                }
            }
        }
        if component != 0 {
            for &(bit, idx) in spread {
                if nhood & bit != 0 && self.l[idx] == 0 {
                    self.l[idx] = component;
                    self.count(component);
                }
            }
        }
    }

    /// Returns the (A, B) vote contributions.
    fn run(mut self, sim: &Graph, x: i32, y: i32) -> (i32, i32) {
        let ix = |r: i32, c: i32| (8 * r + c) as usize;
        for level in 0..2i32 {
            let l = level;
            let o = 1 + 2 * l;

            // Upper left corner of this ring, then its top edge.
            let nhood = sim.get_g(x - o, y + o);
            self.visit(
                nhood,
                ix(3 - l, 3 - l),
                &[
                    same(SOUTH, ix(4 - l, 3 - l)),
                    same(SOUTHWEST, ix(4 - l, 2 - l)),
                    same(WEST, ix(3 - l, 2 - l)),
                    same(NORTHWEST, ix(2 - l, 2 - l)),
                    same(NORTH, ix(2 - l, 3 - l)),
                    same(NORTHEAST, ix(2 - l, 4 - l)),
                    same(EAST, ix(3 - l, 4 - l)),
                ],
                &[
                    (SOUTHWEST, ix(4 - l, 2 - l)),
                    (WEST, ix(3 - l, 2 - l)),
                    (NORTHWEST, ix(2 - l, 2 - l)),
                    (NORTH, ix(2 - l, 3 - l)),
                    (NORTHEAST, ix(2 - l, 4 - l)),
                ],
            );
            for i in 0..2 * l {
                let nhood = sim.get_g(x - (2 * l - 1) + 2 * i, y + o);
                self.visit(
                    nhood,
                    ix(3 - l, i + 4 - l),
                    &[
                        same(WEST, ix(3 - l, i + 3 - l)),
                        same(NORTHWEST, ix(2 - l, i + 3 - l)),
                        same(NORTH, ix(2 - l, i + 4 - l)),
                        same(NORTHEAST, ix(2 - l, i + 5 - l)),
                        same(EAST, ix(3 - l, i + 5 - l)),
                    ],
                    &[
                        (NORTHWEST, ix(2 - l, i + 3 - l)),
                        (NORTH, ix(2 - l, i + 4 - l)),
                        (NORTHEAST, ix(2 - l, i + 5 - l)),
                    ],
                );
            }

            // Upper right corner, then its right edge.
            let nhood = sim.get_g(x + o, y + o);
            // The first cascade entry tests the north-west cell but takes
            // the west one, exactly as in the JavaScript.
            self.visit(
                nhood,
                ix(3 - l, 4 + l),
                &[
                    (WEST, ix(2 - l, 3 + l), ix(3 - l, 3 + l)),
                    same(NORTHWEST, ix(2 - l, 3 + l)),
                    same(NORTH, ix(2 - l, 4 + l)),
                    same(NORTHEAST, ix(2 - l, 5 + l)),
                    same(EAST, ix(3 - l, 5 + l)),
                    same(SOUTHEAST, ix(4 - l, 5 + l)),
                    same(SOUTH, ix(4 - l, 4 + l)),
                ],
                &[
                    (NORTHWEST, ix(2 - l, 3 + l)),
                    (NORTH, ix(2 - l, 4 + l)),
                    (NORTHEAST, ix(2 - l, 5 + l)),
                    (EAST, ix(3 - l, 5 + l)),
                    (SOUTHEAST, ix(4 - l, 5 + l)),
                ],
            );
            for i in 0..2 * l {
                let nhood = sim.get_g(x + o, y + 2 * l - 1 - 2 * i);
                self.visit(
                    nhood,
                    ix(i + 4 - l, 4 + l),
                    &[
                        same(NORTH, ix(i + 3 - l, 4 + l)),
                        same(NORTHEAST, ix(i + 3 - l, 5 + l)),
                        same(EAST, ix(i + 4 - l, 5 + l)),
                        same(SOUTHEAST, ix(i + 5 - l, 5 + l)),
                        same(SOUTH, ix(i + 5 - l, 4 + l)),
                    ],
                    &[
                        (NORTHEAST, ix(i + 3 - l, 5 + l)),
                        (EAST, ix(i + 4 - l, 5 + l)),
                        (SOUTHEAST, ix(i + 5 - l, 5 + l)),
                    ],
                );
            }

            // Lower right corner, then its bottom edge.
            let nhood = sim.get_g(x + o, y - o);
            self.visit(
                nhood,
                ix(4 + l, 4 + l),
                &[
                    same(NORTH, ix(3 + l, 4 + l)),
                    same(NORTHEAST, ix(3 + l, 5 + l)),
                    same(EAST, ix(4 + l, 5 + l)),
                    same(SOUTHEAST, ix(5 + l, 5 + l)),
                    same(SOUTH, ix(5 + l, 4 + l)),
                    same(SOUTHWEST, ix(5 + l, 3 + l)),
                    same(WEST, ix(4 + l, 3 + l)),
                ],
                &[
                    (NORTHEAST, ix(3 + l, 5 + l)),
                    (EAST, ix(4 + l, 5 + l)),
                    (SOUTHEAST, ix(5 + l, 5 + l)),
                    (SOUTH, ix(5 + l, 4 + l)),
                    (SOUTHWEST, ix(5 + l, 3 + l)),
                ],
            );
            for i in 0..2 * l {
                let nhood = sim.get_g(x - (2 * l - 1) + 2 * i, y - o);
                self.visit(
                    nhood,
                    ix(4 + l, i + 4 - l),
                    &[
                        same(EAST, ix(4 + l, i + 5 - l)),
                        same(SOUTHEAST, ix(5 + l, i + 5 - l)),
                        same(SOUTH, ix(5 + l, i + 4 - l)),
                        same(SOUTHWEST, ix(5 + l, i + 3 - l)),
                        same(WEST, ix(4 + l, i + 3 - l)),
                    ],
                    &[
                        (EAST, ix(4 + l, i + 5 - l)),
                        (SOUTHEAST, ix(5 + l, i + 5 - l)),
                        (SOUTH, ix(5 + l, i + 4 - l)),
                        (SOUTHWEST, ix(5 + l, i + 3 - l)),
                        (WEST, ix(4 + l, i + 3 - l)),
                    ],
                );
            }

            // Lower left corner, then its left edge.
            let nhood = sim.get_g(x - o, y - o);
            self.visit(
                nhood,
                ix(4 + l, 3 - l),
                &[
                    same(NORTH, ix(3 + l, 3 - l)),
                    same(NORTHWEST, ix(3 + l, 2 - l)),
                    same(WEST, ix(4 + l, 2 - l)),
                    same(SOUTHWEST, ix(5 + l, 2 - l)),
                    same(SOUTH, ix(5 + l, 3 - l)),
                    same(SOUTHEAST, ix(5 + l, 4 - l)),
                    same(EAST, ix(4 + l, 4 - l)),
                ],
                &[
                    (NORTH, ix(3 + l, 3 - l)),
                    (NORTHWEST, ix(3 + l, 2 - l)),
                    (WEST, ix(4 + l, 2 - l)),
                    (SOUTHWEST, ix(5 + l, 2 - l)),
                    (SOUTH, ix(5 + l, 3 - l)),
                ],
            );
            for i in 0..2 * l {
                let nhood = sim.get_g(x - o, y + 2 * l - 1 - 2 * i);
                self.visit(
                    nhood,
                    ix(i + 4 - l, 3 - l),
                    &[
                        same(NORTH, ix(i + 3 - l, 3 - l)),
                        same(NORTHWEST, ix(i + 3 - l, 2 - l)),
                        same(WEST, ix(i + 4 - l, 2 - l)),
                        same(SOUTHWEST, ix(i + 5 - l, 2 - l)),
                        same(SOUTH, ix(i + 5 - l, 3 - l)),
                    ],
                    &[
                        (NORTH, ix(i + 3 - l, 3 - l)),
                        (NORTHWEST, ix(i + 3 - l, 2 - l)),
                        (WEST, ix(i + 4 - l, 2 - l)),
                        (SOUTHWEST, ix(i + 5 - l, 2 - l)),
                        (SOUTH, ix(i + 5 - l, 3 - l)),
                    ],
                );
            }
        }

        // The smaller component loses: the bigger one gets the difference.
        match self.size_a.cmp(&self.size_b) {
            std::cmp::Ordering::Less => (self.size_b - self.size_a, 0),
            std::cmp::Ordering::Greater => (0, self.size_a - self.size_b),
            std::cmp::Ordering::Equal => (0, 0),
        }
    }
}

/// Follows a chain of valence-2 nodes and returns its length; 0 if the
/// chain hits a direction that cannot be followed.
fn trace_nodes(sim: &Graph, start: (i32, i32), mut predecessor: i32) -> i32 {
    let mut total = 0;
    let mut cur = start;
    let mut vx = sim.get_r(cur.0, cur.1);
    let mut vy = sim.get_g(cur.0, cur.1);
    while vx == 2 {
        let (next, back) = match vy ^ predecessor {
            1 => ((cur.0 - 1, cur.1 + 1), 16),
            2 => ((cur.0 - 1, cur.1), 32),
            4 => ((cur.0 - 1, cur.1 - 1), 64),
            8 => ((cur.0, cur.1 - 1), 128),
            16 => ((cur.0 + 1, cur.1 - 1), 1),
            32 => ((cur.0 + 1, cur.1), 2),
            64 => ((cur.0 + 1, cur.1 + 1), 4),
            128 => ((cur.0, cur.1 + 1), 8),
            _ => return 0,
        };
        cur = next;
        predecessor = back;
        vx = sim.get_r(cur.0, cur.1);
        vy = sim.get_g(cur.0, cur.1);
        total += 1;
    }
    total
}

/// Decides which diagonal survives where both cross (value 96), by the
/// paper's three heuristics: curve length, islands and sparse pixels.
/// Returns the new edge value.
fn resolve_crossing(sim: &Graph, x: i32, y: i32) -> i32 {
    // Fully connected block: both diagonals go.
    if sim.get_r(x, y + 1) != 0 {
        return 0;
    }
    let mut vote_a = 0;
    let mut vote_b = 0;

    // Curves: the longer curve wins.
    let mut length_a = 1;
    let mut length_b = 1;
    length_a += trace_nodes(sim, (x - 1, y + 1), 16);
    length_a += trace_nodes(sim, (x + 1, y - 1), 1);
    length_b += trace_nodes(sim, (x - 1, y - 1), 64);
    length_b += trace_nodes(sim, (x + 1, y + 1), 4);
    if length_a > length_b {
        vote_a += length_a - length_b;
    } else if length_a < length_b {
        vote_b += length_b - length_a;
    }

    // Islands: a diagonal that ends on a lone pixel is worth keeping.
    if sim.get_r(x - 1, y + 1) == 1 || sim.get_r(x + 1, y - 1) == 1 {
        vote_a += 5;
    } else if sim.get_r(x - 1, y - 1) == 1 || sim.get_r(x + 1, y + 1) == 1 {
        vote_b += 5;
    }

    // Sparse pixels: the diagonal whose component is smaller wins.
    let (sa, sb) = Sparse::new().run(sim, x, y);
    vote_a += sa;
    vote_b += sb;

    match vote_a.cmp(&vote_b) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => EDGE_DIAGONAL_ULLR,
        std::cmp::Ordering::Less => EDGE_DIAGONAL_LLUR,
    }
}

fn eliminate_crossings(sim: &Graph) -> Graph {
    let mut out = sim.clone();
    for y in 0..sim.h {
        for x in 0..sim.w {
            let fragment = sim.get_r(x, y);
            if fragment == 96 {
                out.set(x, y, resolve_crossing(sim, x, y), 0);
            } else if fragment == EDGE_DIAGONAL_ULLR || fragment == EDGE_DIAGONAL_LLUR {
                let fully_connected = sim.get_r(x, y + 1) == EDGE_HORVERT
                    && sim.get_r(x + 1, y) == EDGE_HORVERT
                    && sim.get_r(x, y - 1) == EDGE_HORVERT
                    && sim.get_r(x - 1, y) == EDGE_HORVERT;
                if fully_connected {
                    out.set(x, y, 0, 0);
                }
            }
        }
    }
    out
}

/// Two nodes per pixel-corner cell: positions (f32, like the JavaScript's
/// `Float32Array`), the four neighbour node indices (N, E, S, W) and flags.
struct CellGraph {
    pos: Vec<f32>,
    neighbors: Vec<i32>,
    flags: Vec<i32>,
}

impl CellGraph {
    /// Flags of a node; 0 outside the array, like `flags[i] | 0`.
    fn flag(&self, i: i32) -> i32 {
        if i < 0 {
            0
        } else {
            self.flags.get(i as usize).copied().unwrap_or(0)
        }
    }

    /// Neighbour `k` of a node; -1 outside the array.
    fn neighbor(&self, i: i32, k: usize) -> i32 {
        if i < 0 {
            return -1;
        }
        self.neighbors
            .get(i as usize * 4 + k)
            .copied()
            .unwrap_or(-1)
    }

    fn neighbors_of(&self, i: i32) -> [i32; 4] {
        [
            self.neighbor(i, 0),
            self.neighbor(i, 1),
            self.neighbor(i, 2),
            self.neighbor(i, 3),
        ]
    }
}

/// Position of a node in a point array; NaN outside it.
fn get_pos(pos: &[f32], idx: i32) -> P2 {
    if idx < 0 || idx as usize * 2 + 1 >= pos.len() {
        return [f64::NAN, f64::NAN];
    }
    [
        pos[idx as usize * 2] as f64,
        pos[idx as usize * 2 + 1] as f64,
    ]
}

fn calc_adjusted_point(p0: P2, p1: P2, p2: P2) -> P2 {
    [
        0.125 * p0[0] + 0.75 * p1[0] + 0.125 * p2[0],
        0.125 * p0[1] + 0.75 * p1[1] + 0.125 * p2[1],
    ]
}

/// True if two edge directions meet at one of the angles the paper keeps
/// as sharp corners (135, 108.4 and 90 degrees).
fn check_for_corner(s1: P2, s2: P2) -> bool {
    let n1 = js_hypot(s1[0], s1[1]);
    let n2 = js_hypot(s2[0], s2[1]);
    if n1 == 0.0 || n2 == 0.0 {
        return false;
    }
    let dp = (s1[0] / n1) * (s2[0] / n2) + (s1[1] / n1) * (s2[1] / n2);
    (dp > -0.7072 && dp < -0.7070)
        || (dp > -0.3163 && dp < -0.3161)
        || (dp > -0.0001 && dp < 0.0001)
}

/// One node while the cell graph is built.
struct Node {
    pos: P2,
    nb: [i32; 4],
    flags: i32,
}

impl Node {
    fn empty() -> Node {
        Node {
            pos: [-1.0, -1.0],
            nb: [-1; 4],
            flags: 0,
        }
    }
}

fn compute_cell_graph(src: &Src, sim: &Graph) -> CellGraph {
    let (w, h) = (src.w, src.h);
    let dy = w - 1;
    let count = ((w - 1).max(0) * (h - 1).max(0) * 2) as usize;
    let mut cell = CellGraph {
        pos: vec![0.0; count * 2],
        neighbors: vec![0; count * 4],
        flags: vec![0; count],
    };

    let neighbor_index = |cx: i32, cy: i32, dir: char, sector: i32| -> i32 {
        match dir {
            'N' => ((cy + 1) * dy + cx) * 2 + sector,
            'E' => (cy * dy + cx + 1) * 2 + sector,
            'S' => ((cy - 1) * dy + cx) * 2 + sector,
            _ => (cy * dy + cx - 1) * 2 + sector,
        }
    };

    for cy in 0..h - 1 {
        for cx in 0..w - 1 {
            let sx = cx * 2 + 2;
            let sy = cy * 2 + 2;
            let e_center = sim.get_r(sx, sy);
            let e_north = sim.get_r(sx, sy + 1);
            let e_north_center = sim.get_r(sx, sy + 2);
            let e_east = sim.get_r(sx + 1, sy);
            let e_east_center = sim.get_r(sx + 2, sy);
            let e_south = sim.get_r(sx, sy - 1);
            let e_south_center = sim.get_r(sx, sy - 2);
            let e_west = sim.get_r(sx - 1, sy);
            let e_west_center = sim.get_r(sx - 2, sy);

            let mut v0 = Node::empty();
            let mut v1 = Node::empty();

            let ignore_n = cy > h - 3;
            let ignore_e = cx > w - 3;
            let ignore_s = cy < 1;
            let ignore_w = cx < 1;

            let mut n_found = false;
            let mut w_found = false;
            let mut s_found = false;
            let mut e_found = false;
            let mut neighbor_count = 0;
            let (mut n_idx, mut w_idx, mut s_idx, mut e_idx) = (-1, -1, -1, -1);
            let mut n_vec = [0.0, 0.0];
            let mut e_vec = [0.0, 0.0];
            let mut s_vec = [0.0, 0.0];
            let mut w_vec = [0.0, 0.0];

            if !ignore_n && e_north == 0 {
                n_found = true;
                neighbor_count += 1;
                if e_north_center == EDGE_DIAGONAL_ULLR {
                    n_idx = neighbor_index(cx, cy, 'N', 0);
                    n_vec = [-0.25, 0.75];
                } else if e_north_center == EDGE_DIAGONAL_LLUR {
                    n_idx = neighbor_index(cx, cy, 'N', 1);
                    n_vec = [0.25, 0.75];
                } else {
                    n_idx = neighbor_index(cx, cy, 'N', 0);
                    n_vec = [0.0, 1.0];
                }
            }
            if !ignore_w && e_west == 0 {
                w_found = true;
                neighbor_count += 1;
                if e_west_center == EDGE_DIAGONAL_ULLR {
                    w_idx = neighbor_index(cx, cy, 'W', 1);
                    w_vec = [-0.75, 0.25];
                } else if e_west_center == EDGE_DIAGONAL_LLUR {
                    w_idx = neighbor_index(cx, cy, 'W', 1);
                    w_vec = [-0.75, -0.25];
                } else {
                    w_idx = neighbor_index(cx, cy, 'W', 0);
                    w_vec = [-1.0, 0.0];
                }
            }
            if !ignore_s && e_south == 0 {
                s_found = true;
                neighbor_count += 1;
                if e_south_center == EDGE_DIAGONAL_ULLR {
                    s_idx = neighbor_index(cx, cy, 'S', 1);
                    s_vec = [0.25, -0.75];
                } else if e_south_center == EDGE_DIAGONAL_LLUR {
                    s_idx = neighbor_index(cx, cy, 'S', 0);
                    s_vec = [-0.25, -0.75];
                } else {
                    s_idx = neighbor_index(cx, cy, 'S', 0);
                    s_vec = [0.0, -1.0];
                }
            }
            if !ignore_e && e_east == 0 {
                e_found = true;
                neighbor_count += 1;
                if e_east_center == EDGE_DIAGONAL_ULLR {
                    e_idx = neighbor_index(cx, cy, 'E', 0);
                    e_vec = [0.75, -0.25];
                } else if e_east_center == EDGE_DIAGONAL_LLUR {
                    e_idx = neighbor_index(cx, cy, 'E', 0);
                    e_vec = [0.75, 0.25];
                } else {
                    e_idx = neighbor_index(cx, cy, 'E', 0);
                    e_vec = [1.0, 0.0];
                }
            }

            if n_found || w_found || s_found || e_found {
                let ll = (cx, cy);
                let ul = (cx, cy + 1);
                let lr = (cx + 1, cy);
                let ur = (cx + 1, cy + 1);
                let center = [cx as f64 + 0.5, cy as f64 + 0.5];
                let shift = |v: P2, dx: f64, dy: f64| [v[0] + dx, v[1] + dy];

                if e_center == EDGE_DIAGONAL_ULLR {
                    let mut two = true;
                    v0.pos = [center[0] - 0.25, center[1] - 0.25];
                    if s_found {
                        v0.flags = HAS_SOUTHERN_NEIGHBOR | HAS_SOUTHERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if w_found {
                        v0.flags |= HAS_WESTERN_NEIGHBOR | HAS_WESTERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if two && check_for_corner(shift(s_vec, 0.25, 0.25), shift(w_vec, 0.25, 0.25)) {
                        v0.flags |= DONT_OPTIMIZE_S | DONT_OPTIMIZE_W;
                    }
                    v0.nb = [-1, -1, s_idx, w_idx];

                    two = true;
                    v1.pos = [center[0] + 0.25, center[1] + 0.25];
                    if n_found {
                        v1.flags = HAS_NORTHERN_NEIGHBOR | HAS_NORTHERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if e_found {
                        v1.flags |= HAS_EASTERN_NEIGHBOR | HAS_EASTERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if two
                        && check_for_corner(shift(n_vec, -0.25, -0.25), shift(e_vec, -0.25, -0.25))
                    {
                        v1.flags |= DONT_OPTIMIZE_N | DONT_OPTIMIZE_E;
                    }
                    v1.nb = [n_idx, e_idx, -1, -1];
                } else if e_center == EDGE_DIAGONAL_LLUR {
                    let mut two = true;
                    v0.pos = [center[0] - 0.25, center[1] + 0.25];
                    if n_found {
                        v0.flags = HAS_NORTHERN_NEIGHBOR | HAS_NORTHERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if w_found {
                        v0.flags |= HAS_WESTERN_NEIGHBOR | HAS_WESTERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if two && check_for_corner(shift(n_vec, 0.25, -0.25), shift(w_vec, 0.25, -0.25))
                    {
                        v0.flags |= DONT_OPTIMIZE_N | DONT_OPTIMIZE_W;
                    }
                    v0.nb = [n_idx, -1, -1, w_idx];

                    two = true;
                    v1.pos = [center[0] + 0.25, center[1] - 0.25];
                    if s_found {
                        v1.flags = HAS_SOUTHERN_NEIGHBOR | HAS_SOUTHERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if e_found {
                        v1.flags |= HAS_EASTERN_NEIGHBOR | HAS_EASTERN_SPLINE;
                    } else {
                        two = false;
                    }
                    if two && check_for_corner(shift(s_vec, -0.25, 0.25), shift(e_vec, -0.25, 0.25))
                    {
                        v1.flags |= DONT_OPTIMIZE_S | DONT_OPTIMIZE_E;
                    }
                    v1.nb = [-1, e_idx, s_idx, -1];
                } else {
                    v0.pos = center;
                    if n_found {
                        v0.flags |= HAS_NORTHERN_NEIGHBOR;
                    }
                    if e_found {
                        v0.flags |= HAS_EASTERN_NEIGHBOR;
                    }
                    if s_found {
                        v0.flags |= HAS_SOUTHERN_NEIGHBOR;
                    }
                    if w_found {
                        v0.flags |= HAS_WESTERN_NEIGHBOR;
                    }

                    if neighbor_count == 2 {
                        if n_found {
                            v0.flags |= HAS_NORTHERN_SPLINE;
                        }
                        if e_found {
                            v0.flags |= HAS_EASTERN_SPLINE;
                        }
                        if s_found {
                            v0.flags |= HAS_SOUTHERN_SPLINE;
                        }
                        if w_found {
                            v0.flags |= HAS_WESTERN_SPLINE;
                        }
                    } else if neighbor_count == 3 {
                        let mut contours = 0;
                        let mut p: Vec<P2> = Vec::with_capacity(4);
                        if n_found && is_contour(src, ul, ur) {
                            p.push(n_vec);
                            contours |= HAS_NORTHERN_SPLINE;
                        }
                        if e_found && is_contour(src, ur, lr) {
                            p.push(e_vec);
                            contours |= HAS_EASTERN_SPLINE;
                        }
                        if s_found && is_contour(src, lr, ll) {
                            p.push(s_vec);
                            contours |= HAS_SOUTHERN_SPLINE;
                        }
                        if w_found && is_contour(src, ll, ul) {
                            p.push(w_vec);
                            contours |= HAS_WESTERN_SPLINE;
                        }
                        // The two directions the extra, corrected node bends between.
                        let (a, b) = if p.len() == 2 {
                            v0.flags |= contours | HAS_CORRECTED_POSITION;
                            (p[0], p[1])
                        } else if n_found && s_found {
                            v0.flags |=
                                HAS_NORTHERN_SPLINE | HAS_SOUTHERN_SPLINE | HAS_CORRECTED_POSITION;
                            (n_vec, s_vec)
                        } else {
                            v0.flags |=
                                HAS_EASTERN_SPLINE | HAS_WESTERN_SPLINE | HAS_CORRECTED_POSITION;
                            (e_vec, w_vec)
                        };
                        v1.pos = calc_adjusted_point(
                            [center[0] + a[0], center[1] + a[1]],
                            center,
                            [center[0] + b[0], center[1] + b[1]],
                        );
                        v1.flags = -1;
                    }
                    v0.nb = [n_idx, e_idx, s_idx, w_idx];
                }
            }

            let base = (cy * (w - 1) + cx) as usize * 2;
            for (k, v) in [v0, v1].into_iter().enumerate() {
                let n = base + k;
                cell.pos[n * 2] = v.pos[0] as f32;
                cell.pos[n * 2 + 1] = v.pos[1] as f32;
                cell.neighbors[n * 4..n * 4 + 4].copy_from_slice(&v.nb);
                cell.flags[n] = v.flags;
            }
        }
    }
    cell
}

fn spline_energy(n0: P2, p: P2, n1: P2, old: P2) -> f64 {
    let tx = n0[0] - 2.0 * p[0] + n1[0];
    let ty = n0[1] - 2.0 * p[1] + n1[1];
    let dx = p[0] - old[0];
    let dy = p[1] - old[1];
    let dist_sq = POSITIONAL_ENERGY_SCALING * POSITIONAL_ENERGY_SCALING * (dx * dx + dy * dy);
    (tx * tx + ty * ty) + dist_sq * dist_sq
}

/// Moves one control point along the curvature gradient to the minimum of
/// the curve energy plus a pull back to where it started (golden-section
/// line search after a bracketing step).
fn search_offset(pos: P2, nb: [P2; 2]) -> [f64; 3] {
    let mut gradient = [
        8.0 * pos[0] - 4.0 * nb[0][0] - 4.0 * nb[1][0],
        8.0 * pos[1] - 4.0 * nb[0][1] - 4.0 * nb[1][1],
    ];
    let glen = js_hypot(gradient[0], gradient[1]);
    if glen <= 0.0 {
        return [0.0, 0.0, 0.0];
    }
    gradient = [gradient[0] / glen, gradient[1] / glen];
    let f = |t: f64| {
        let p = [pos[0] - gradient[0] * t, pos[1] - gradient[1] * t];
        spline_energy(nb[0], p, nb[1], pos)
    };

    let bracket = find_bracket(&f);
    let mut x0 = bracket[0];
    let mut x3 = bracket[2];
    let (mut x1, mut x2);
    if (bracket[2] - bracket[1]).abs() > (bracket[1] - bracket[0]).abs() {
        x1 = bracket[1];
        x2 = bracket[1] + C * (bracket[2] - bracket[1]);
    } else {
        x1 = bracket[1] - C * (bracket[1] - bracket[0]);
        x2 = bracket[1];
    }
    let mut f1 = f(x1);
    let mut f2 = f(x2);
    let mut counter = 0;
    while (x3 - x0).abs() > TOL * (x1.abs() + x2.abs()) && counter < LIMIT_SEARCH_ITERATIONS {
        counter += 1;
        if f2 < f1 {
            x0 = x1;
            x1 = x2;
            x2 = R * x1 + C * x3;
            let fx = f(x2);
            f1 = f2;
            f2 = fx;
        } else {
            x3 = x2;
            x2 = x1;
            x1 = R * x2 + C * x0;
            let fx = f(x1);
            f2 = f1;
            f1 = fx;
        }
    }
    let offset = if f1 < f2 { x1 } else { x2 };
    [gradient[0], gradient[1], offset]
}

/// Brackets a minimum of `f` (Numerical Recipes `mnbrak` as the JavaScript
/// has it, including its early returns).
fn find_bracket(f: &impl Fn(f64) -> f64) -> [f64; 3] {
    let mut ax = BRACKET_SEARCH_A;
    let mut bx = BRACKET_SEARCH_B;
    let mut fa = f(ax);
    let mut fb = f(bx);
    if fb > fa {
        std::mem::swap(&mut ax, &mut bx);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut cx = bx + GOLD * (bx - ax);
    let mut fc = f(cx);
    while fb > fc {
        let r = (bx - ax) * (fb - fc);
        let q = (bx - cx) * (fb - fa);
        let qr = q - r;
        let mut u =
            bx - ((bx - cx) * q - (bx - ax) * r) / (2.0 * js_sign(qr) * js_max(qr.abs(), TINY));
        let ulim = bx + GLIMIT * (cx - bx);
        let mut fu;
        if (bx - u) * (u - cx) > 0.0 {
            fu = f(u);
            if fu < fc {
                return [bx, u, cx];
            }
            if fu > fb {
                return [ax, bx, u];
            }
            u = cx + GOLD * (cx - bx);
            fu = f(u);
        } else if (cx - u) * (u - ulim) > 0.0 {
            fu = f(u);
            if fu < fc {
                let dum = cx + GOLD * (cx - bx);
                bx = cx;
                cx = u;
                u = dum;
                fb = fc;
                fc = fu;
                fu = f(u);
            }
        } else if (u - ulim) * (ulim - cx) >= 0.0 {
            u = ulim;
            fu = f(u);
        } else {
            u = cx + GOLD * (cx - bx);
            fu = f(u);
        }
        ax = bx;
        bx = cx;
        cx = u;
        fa = fb;
        fb = fc;
        fc = fu;
    }
    [ax, bx, cx]
}

/// Smooths the spline control points (nodes with exactly two splines).
fn optimize_cell_graph(cell: &CellGraph) -> Vec<f32> {
    let mut optimized = cell.pos.clone();

    for i in 0..cell.flags.len() {
        let flags = cell.flags[i];
        if flags & ANY_SPLINE == 0 {
            continue;
        }
        let nb = cell.neighbors_of(i as i32);
        let pos = get_pos(&optimized, i as i32);

        let mut spline_nb: Vec<P2> = Vec::with_capacity(2);
        let mut no_opt = false;

        // (this node has the spline, its don't-optimise bit, the neighbour's
        // opposing don't-optimise bit, the neighbour's opposing spline bit)
        let sides = [
            (
                HAS_NORTHERN_SPLINE,
                DONT_OPTIMIZE_N,
                DONT_OPTIMIZE_S,
                HAS_SOUTHERN_SPLINE,
                nb[0],
            ),
            (
                HAS_EASTERN_SPLINE,
                DONT_OPTIMIZE_E,
                DONT_OPTIMIZE_W,
                HAS_WESTERN_SPLINE,
                nb[1],
            ),
            (
                HAS_SOUTHERN_SPLINE,
                DONT_OPTIMIZE_S,
                DONT_OPTIMIZE_N,
                HAS_NORTHERN_SPLINE,
                nb[2],
            ),
            (
                HAS_WESTERN_SPLINE,
                DONT_OPTIMIZE_W,
                DONT_OPTIMIZE_E,
                HAS_EASTERN_SPLINE,
                nb[3],
            ),
        ];
        for (has, own_no_opt, their_no_opt, their_spline, n) in sides {
            if flags & has == 0 {
                continue;
            }
            let nflags = cell.flag(n);
            if flags & own_no_opt != 0 || nflags & their_no_opt != 0 {
                no_opt = true;
            }
            if !no_opt {
                let idx = if nflags & HAS_CORRECTED_POSITION != 0 && nflags & their_spline == 0 {
                    n + 1
                } else {
                    n
                };
                spline_nb.push(get_pos(&cell.pos, idx));
            }
        }

        if spline_nb.len() == 2 && !no_opt {
            let shift = search_offset(pos, [spline_nb[0], spline_nb[1]]);
            optimized[i * 2] = (pos[0] - shift[0] * shift[2]) as f32;
            optimized[i * 2 + 1] = (pos[1] - shift[1] * shift[2]) as f32;
        }
    }
    optimized
}

/// Positions of the extra (flag -1) nodes: the parent's spline neighbours
/// pull them to the 1/8, 3/4, 1/8 weighted point.
fn compute_corrected_positions(cell: &CellGraph, optimized: &[f32]) -> Vec<f32> {
    let mut corrected = optimized.to_vec();
    for i in 0..cell.flags.len() {
        if cell.flags[i] != -1 {
            continue;
        }
        let parent = i as i32 - 1;
        let parent_pos = get_pos(optimized, parent);
        let parent_flags = cell.flag(parent);
        let nb = cell.neighbors_of(parent);
        let mut points: Vec<P2> = Vec::with_capacity(4);
        for (bit, n) in [
            (HAS_NORTHERN_SPLINE, nb[0]),
            (HAS_EASTERN_SPLINE, nb[1]),
            (HAS_SOUTHERN_SPLINE, nb[2]),
            (HAS_WESTERN_SPLINE, nb[3]),
        ] {
            if parent_flags & bit != 0 {
                points.push(get_pos(optimized, n));
            }
        }
        let p = if points.len() == 2 {
            calc_adjusted_point(points[0], parent_pos, points[1])
        } else {
            parent_pos
        };
        corrected[i * 2] = p[0] as f32;
        corrected[i * 2 + 1] = p[1] as f32;
    }
    corrected
}

fn calc_spline_point(p0: P2, p1: P2, p2: P2, t: f64) -> P2 {
    let t2 = 0.5 * t * t;
    let a = t2 - t + 0.5;
    let b = -2.0 * t2 + t + 0.5;
    [
        a * p0[0] + b * p1[0] + t2 * p2[0],
        a * p0[1] + b * p1[1] + t2 * p2[1],
    ]
}

/// Segment intersection test. NaN coordinates make it answer true, as the
/// JavaScript does.
fn intersects(a0: P2, a1: P2, b0: P2, b1: P2) -> bool {
    let r = [a1[0] - a0[0], a1[1] - a0[1]];
    let s = [b1[0] - b0[0], b1[1] - b0[1]];
    let r_x_s = r[0] * s[1] - r[1] * s[0];
    if r_x_s == 0.0 {
        return false;
    }
    let ba = [b0[0] - a0[0], b0[1] - a0[1]];
    let t = (ba[0] * s[1] - ba[1] * s[0]) / r_x_s;
    if t < 0.0 || t > 1.0 {
        return false;
    }
    let u = (ba[0] * r[1] - ba[1] * r[0]) / r_x_s;
    !(u < 0.0 || u > 1.0)
}

fn valence_of(flags: i32) -> u32 {
    [
        HAS_NORTHERN_NEIGHBOR,
        HAS_EASTERN_NEIGHBOR,
        HAS_SOUTHERN_NEIGHBOR,
        HAS_WESTERN_NEIGHBOR,
    ]
    .iter()
    .filter(|&&b| flags & b != 0)
    .count() as u32
}

/// The spline control points that continue a curve from a neighbour node:
/// `[first, second]` node indices, second -1 when the curve ends there.
fn get_cps(cell: &CellGraph, n0: i32, dir: i32) -> [i32; 2] {
    let mut cp = [n0, -1];
    let (fwd, back, chk) = match dir {
        NORTH => (
            [HAS_NORTHERN_SPLINE, HAS_EASTERN_SPLINE, HAS_WESTERN_SPLINE],
            HAS_SOUTHERN_SPLINE,
            [EAST, WEST],
        ),
        EAST => (
            [HAS_EASTERN_SPLINE, HAS_SOUTHERN_SPLINE, HAS_NORTHERN_SPLINE],
            HAS_WESTERN_SPLINE,
            [SOUTH, NORTH],
        ),
        SOUTH => (
            [HAS_SOUTHERN_SPLINE, HAS_WESTERN_SPLINE, HAS_EASTERN_SPLINE],
            HAS_NORTHERN_SPLINE,
            [WEST, EAST],
        ),
        WEST => (
            [HAS_WESTERN_SPLINE, HAS_NORTHERN_SPLINE, HAS_SOUTHERN_SPLINE],
            HAS_EASTERN_SPLINE,
            [NORTH, SOUTH],
        ),
        _ => ([0, 0, 0], 0, [0, 0]),
    };
    let neighbor_in = |node: i32, d: i32| -> i32 {
        let k = match d {
            NORTH => 0,
            EAST => 1,
            SOUTH => 2,
            WEST => 3,
            _ => return -1,
        };
        cell.neighbor(node, k)
    };
    let n0_flags = cell.flag(n0);
    if n0_flags & back == back {
        let via = if n0_flags & fwd[0] == fwd[0] {
            Some(dir)
        } else if n0_flags & fwd[1] == fwd[1] {
            Some(chk[0])
        } else if n0_flags & fwd[2] == fwd[2] {
            Some(chk[1])
        } else {
            None
        };
        if let Some(d) = via {
            let nn = neighbor_in(n0, d);
            cp[1] = if cell.flag(nn) & HAS_CORRECTED_POSITION != 0 {
                nn + 1
            } else {
                nn
            };
        }
    } else if n0_flags & HAS_CORRECTED_POSITION != 0 {
        cp[0] += 1;
    }
    cp
}

/// Per output pixel: which of the four surrounding source pixels can still
/// see this pixel, i.e. are not cut off by an edge spline.
struct Probe {
    center: P2,
    corners: [P2; 4],
    influencing: [bool; 4],
}

impl Probe {
    fn find(&mut self, p0: P2, p1: P2, p2: P2) {
        let mut a = calc_spline_point(p0, p1, p2, 0.0);
        let mut t = STEP;
        while t < 1.0 + STEP {
            let b = calc_spline_point(p0, p1, p2, t);
            for k in 0..4 {
                if self.influencing[k] && intersects(self.center, self.corners[k], a, b) {
                    self.influencing[k] = false;
                }
            }
            a = b;
            t += STEP;
        }
    }

    /// The spline starting at `node` and running through `p1`, then the one
    /// continuing to the second control point (or ending at `p1`).
    fn tail(&mut self, node: P2, p1: P2, pos: &[f32], cp1: i32) {
        if cp1 > -1 {
            self.find(node, p1, get_pos(pos, cp1));
        } else {
            self.find(node, p1, p1);
        }
    }
}

/// The curves leaving a node in the given neighbour directions, in N, E, S,
/// W order. With `only_splines` neighbours joined without a spline are
/// skipped and the last one seen is returned as the base direction.
fn gather_cps(
    cell: &CellGraph,
    flags: i32,
    nb: [i32; 4],
    only_splines: bool,
) -> ([i32; 4], i32, i32) {
    let mut cp = [-1; 4];
    let mut found_first = false;
    let mut base_dir = 0;
    let mut base_neighbor = -1;
    let sides = [
        (HAS_NORTHERN_NEIGHBOR, HAS_NORTHERN_SPLINE, NORTH, nb[0]),
        (HAS_EASTERN_NEIGHBOR, HAS_EASTERN_SPLINE, EAST, nb[1]),
        (HAS_SOUTHERN_NEIGHBOR, HAS_SOUTHERN_SPLINE, SOUTH, nb[2]),
        (HAS_WESTERN_NEIGHBOR, HAS_WESTERN_SPLINE, WEST, nb[3]),
    ];
    for (neighbor_bit, spline_bit, dir, n) in sides {
        if flags & neighbor_bit == 0 {
            continue;
        }
        if only_splines && flags & spline_bit == 0 {
            base_dir = dir;
            base_neighbor = n;
            continue;
        }
        let tmp = get_cps(cell, n, dir);
        if found_first || dir == WEST {
            cp[2] = tmp[0];
            cp[3] = tmp[1];
        } else {
            cp[0] = tmp[0];
            cp[1] = tmp[1];
            found_first = true;
        }
    }
    (cp, base_dir, base_neighbor)
}

/// The rasteriser: for each output pixel, a Gaussian-weighted mean of the
/// source pixels around it that are not separated from it by an edge.
fn gauss_rasterize(
    src: &Src,
    sim: &Graph,
    cell: &CellGraph,
    positions: &[f32],
    out_w: i32,
    out_h: i32,
) -> Vec<u8> {
    let (w, h) = (src.w, src.h);
    let mut out = vec![0u8; out_w as usize * out_h as usize * 4];

    for oy in 0..out_h {
        for ox in 0..out_w {
            let csx = (w as f64 * (ox as f64 + 0.5) / out_w as f64) - 0.5 + 0.00001;
            let csy = (h as f64 * (oy as f64 + 0.5) / out_h as f64) - 0.5 + 0.00001;
            let (fx, fy) = (csx.floor(), csy.floor());
            let (cxl, cyl) = (fx as i32, fy as i32);
            let (cxh, cyh) = (csx.ceil() as i32, csy.ceil() as i32);
            let ul = (cxl, cyh);
            let ur = (cxh, cyh);
            let ll = (cxl, cyl);
            let lr = (cxh, cyl);
            let base = (2.0 * fx + fy * 2.0 * (w - 1) as f64) as i32;

            let mut probe = Probe {
                center: [csx, csy],
                corners: [
                    [ul.0 as f64, ul.1 as f64],
                    [ur.0 as f64, ur.1 as f64],
                    [ll.0 as f64, ll.1 as f64],
                    [lr.0 as f64, lr.1 as f64],
                ],
                influencing: [true; 4],
            };
            let pos = |idx: i32| get_pos(positions, idx);

            let mut has_corrected = false;
            let n0_flags = cell.flag(base);
            if n0_flags > 0 {
                let n0_nb = cell.neighbors_of(base);
                let n0_pos = pos(base);
                match valence_of(n0_flags) {
                    1 => {
                        let cp = single_cp(cell, n0_flags, n0_nb);
                        let p1 = pos(cp[0]);
                        probe.find(n0_pos, n0_pos, p1);
                        probe.tail(n0_pos, p1, positions, cp[1]);
                    }
                    2 => {
                        let (cp, _, _) = gather_cps(cell, n0_flags, n0_nb, false);
                        two_sided(&mut probe, n0_pos, cp, positions);
                    }
                    3 => {
                        has_corrected = true;
                        let (cp, base_dir, base_neighbor) = gather_cps(cell, n0_flags, n0_nb, true);
                        two_sided(&mut probe, n0_pos, cp, positions);
                        // The third branch hangs off the extra node.
                        let t_cp = get_cps(cell, base_neighbor, base_dir);
                        let extra = pos(base + 1);
                        let p1 = pos(t_cp[0]);
                        probe.find(extra, extra, p1);
                        probe.tail(extra, p1, positions, t_cp[1]);
                    }
                    _ => {
                        for (n, dir) in [
                            (n0_nb[0], NORTH),
                            (n0_nb[1], EAST),
                            (n0_nb[2], SOUTH),
                            (n0_nb[3], WEST),
                        ] {
                            let cp = get_cps(cell, n, dir);
                            let p1 = pos(cp[0]);
                            probe.find(n0_pos, n0_pos, p1);
                            probe.tail(n0_pos, p1, positions, cp[1]);
                        }
                    }
                }
            }

            if !has_corrected {
                let n1_flags = cell.flag(base + 1);
                if n1_flags > 0 {
                    let n1_nb = cell.neighbors_of(base + 1);
                    let n1_pos = pos(base + 1);
                    match valence_of(n1_flags) {
                        1 => {
                            let cp = single_cp(cell, n1_flags, n1_nb);
                            let p1 = pos(cp[0]);
                            probe.find(n1_pos, n1_pos, p1);
                            probe.tail(n1_pos, p1, positions, cp[1]);
                        }
                        2 => {
                            let (cp, _, _) = gather_cps(cell, n1_flags, n1_nb, false);
                            two_sided(&mut probe, n1_pos, cp, positions);
                        }
                        _ => {}
                    }
                }
            }

            let mut color_sum = [0.0f64; 4];
            let mut weight_sum = 0.0f64;
            let mut add = |px: i32, py: i32| {
                let col = src.px8(px, py);
                let dx = csx - px as f64;
                let dy = csy - py as f64;
                let weight = (-(dx * dx + dy * dy) * GAUSS_MULTIPLIER).exp();
                for k in 0..4 {
                    color_sum[k] += col[k] as f64 * weight;
                }
                weight_sum += weight;
            };

            if probe.influencing[0] {
                add(ul.0, ul.1);
                let edges = sim.get_g(2 * ul.0 + 1, 2 * ul.1 + 1);
                if edges & SOUTHWEST != 0 {
                    add(ul.0 - 1, ul.1 - 1);
                }
                if edges & WEST != 0 {
                    add(ul.0 - 1, ul.1);
                }
                if edges & NORTHWEST != 0 {
                    add(ul.0 - 1, ul.1 + 1);
                }
                if edges & NORTH != 0 {
                    add(ul.0, ul.1 + 1);
                }
                if edges & NORTHEAST != 0 {
                    add(ul.0 + 1, ul.1 + 1);
                }
            }
            if probe.influencing[1] {
                add(ur.0, ur.1);
                let edges = sim.get_g(2 * ur.0 + 1, 2 * ur.1 + 1);
                if edges & NORTH != 0 {
                    add(ur.0, ur.1 + 1);
                }
                if edges & NORTHEAST != 0 {
                    add(ur.0 + 1, ur.1 + 1);
                }
                if edges & EAST != 0 {
                    add(ur.0 + 1, ur.1);
                }
                if edges & SOUTHEAST != 0 {
                    add(ur.0 + 1, ur.1 - 1);
                }
            }
            if probe.influencing[2] {
                add(ll.0, ll.1);
                let edges = sim.get_g(2 * ll.0 + 1, 2 * ll.1 + 1);
                if edges & WEST != 0 {
                    add(ll.0 - 1, ll.1);
                }
                if edges & SOUTHWEST != 0 {
                    add(ll.0 - 1, ll.1 - 1);
                }
                if edges & SOUTH != 0 {
                    add(ll.0, ll.1 - 1);
                }
                if edges & SOUTHEAST != 0 {
                    add(ll.0 + 1, ll.1 - 1);
                }
            }
            if probe.influencing[3] {
                add(lr.0, lr.1);
                let edges = sim.get_g(2 * lr.0 + 1, 2 * lr.1 + 1);
                if edges & NORTHEAST != 0 {
                    add(lr.0 + 1, lr.1 + 1);
                }
                if edges & EAST != 0 {
                    add(lr.0 + 1, lr.1);
                }
                if edges & SOUTHWEST != 0 {
                    add(lr.0 - 1, lr.1 - 1);
                }
                if edges & SOUTH != 0 {
                    add(lr.0, lr.1 - 1);
                }
                if edges & SOUTHEAST != 0 {
                    add(lr.0 + 1, lr.1 - 1);
                }
            }

            let o = (oy as usize * out_w as usize + ox as usize) * 4;
            if weight_sum == 0.0 {
                let col = src.px8(js_round(csx) as i32, js_round(csy) as i32);
                out[o..o + 4].copy_from_slice(&col);
            } else {
                for k in 0..4 {
                    out[o + k] = js_round(color_sum[k] / weight_sum).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

/// Control points for a node with a single neighbour.
fn single_cp(cell: &CellGraph, flags: i32, nb: [i32; 4]) -> [i32; 2] {
    if flags & HAS_NORTHERN_NEIGHBOR != 0 {
        get_cps(cell, nb[0], NORTH)
    } else if flags & HAS_EASTERN_NEIGHBOR != 0 {
        get_cps(cell, nb[1], EAST)
    } else if flags & HAS_SOUTHERN_NEIGHBOR != 0 {
        get_cps(cell, nb[2], SOUTH)
    } else if flags & HAS_WESTERN_NEIGHBOR != 0 {
        get_cps(cell, nb[3], WEST)
    } else {
        [-1, -1]
    }
}

/// The three splines of a node with curves on two sides: the one through
/// the node and one for each side's continuation.
fn two_sided(probe: &mut Probe, node: P2, cp: [i32; 4], positions: &[f32]) {
    let pm1 = get_pos(positions, cp[0]);
    let p1 = get_pos(positions, cp[2]);
    probe.find(pm1, node, p1);
    probe.tail(node, pm1, positions, cp[1]);
    probe.tail(node, p1, positions, cp[3]);
}

/// Scales `img` by an integer `factor` with the default similarity
/// threshold. The result is exactly `w * factor` by `h * factor`.
pub fn scale(img: &RgbaImage, factor: u32) -> RgbaImage {
    scale_with_threshold(img, factor, 255)
}

/// Like [`scale`], with the colour similarity threshold (0..=255, lower
/// joins fewer pixels; 255 is the default).
pub fn scale_with_threshold(img: &RgbaImage, factor: u32, threshold: u8) -> RgbaImage {
    assert!(factor >= 1, "scale factor must be at least 1");
    let (w, h) = img.dimensions();
    assert!(w > 0 && h > 0, "empty image");
    let (out_w, out_h) = (w * factor, h * factor);
    // The JavaScript derives the width from the aspect ratio; for an exact
    // integer factor that is the same number.
    debug_assert_eq!(
        (w as f64 / h as f64 * out_h as f64).round().max(1.0) as u32,
        out_w
    );

    let src = Src {
        data: img.as_raw(),
        w: w as i32,
        h: h as i32,
    };
    let sim0 = build_similarity_graph(&src, threshold as f64 / 255.0);
    let sim1 = valence_update(&sim0);
    let sim2 = eliminate_crossings(&sim1);
    let sim3 = valence_update(&sim2);

    let cell = compute_cell_graph(&src, &sim3);
    let optimized = optimize_cell_graph(&cell);
    let corrected = compute_corrected_positions(&cell, &optimized);

    let data = gauss_rasterize(&src, &sim3, &cell, &corrected, out_w as i32, out_h as i32);
    RgbaImage::from_raw(out_w, out_h, data).expect("output buffer size matches")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic images made for these tests (no game art) and the output of
    // depixel-js (commit b85713c, Node) on them at 4x: `line` is a 1-pixel
    // diagonal, `blob` a two-colour disc on transparent, `stairs` a mix of
    // a staircase edge, 2x2 checker blocks, a pixel checker with a stray
    // pixel, and a small disc.
    const CASES: [(&str, &[u8], &[u8]); 3] = [
        (
            "line",
            include_bytes!("../tests/fixtures/depixel/line.png"),
            include_bytes!("../tests/fixtures/depixel/line.x4.png"),
        ),
        (
            "blob",
            include_bytes!("../tests/fixtures/depixel/blob.png"),
            include_bytes!("../tests/fixtures/depixel/blob.x4.png"),
        ),
        (
            "stairs",
            include_bytes!("../tests/fixtures/depixel/stairs.png"),
            include_bytes!("../tests/fixtures/depixel/stairs.x4.png"),
        ),
    ];

    fn load(png: &[u8]) -> RgbaImage {
        image::load_from_memory(png)
            .expect("fixture decodes")
            .to_rgba8()
    }

    /// Fraction of pixels where every channel is within `tol` of the
    /// reference.
    fn match_fraction(a: &RgbaImage, b: &RgbaImage, tol: i32) -> f64 {
        assert_eq!(a.dimensions(), b.dimensions());
        let ok = a
            .pixels()
            .zip(b.pixels())
            .filter(|(p, q)| {
                p.0.iter()
                    .zip(q.0)
                    .all(|(&x, y)| (x as i32 - y as i32).abs() <= tol)
            })
            .count();
        ok as f64 / (a.width() * a.height()) as f64
    }

    #[test]
    fn matches_depixel_js_exactly() {
        // Same maths as the JavaScript, so we expect every byte to match;
        // the tolerance only covers libm `exp` differing in the last bit
        // before rounding: at least 99.9% of pixels within +-1 per channel.
        for (name, input, expected) in CASES {
            let out = scale(&load(input), 4);
            let expected = load(expected);
            let exact = match_fraction(&out, &expected, 0);
            let near = match_fraction(&out, &expected, 1);
            eprintln!("{name}: exact {exact:.6}, within +-1 {near:.6}");
            assert!(near >= 0.999, "{name}: only {near} of pixels within +-1");
            assert_eq!(exact, 1.0, "{name}: output differs from depixel-js");
        }
    }

    #[test]
    fn output_is_exactly_factor_times_input() {
        let img = load(CASES[1].1);
        for factor in [1, 2, 3, 5] {
            let out = scale(&img, factor);
            assert_eq!(
                out.dimensions(),
                (img.width() * factor, img.height() * factor)
            );
        }
    }

    #[test]
    fn flat_colour_stays_flat() {
        let img = RgbaImage::from_pixel(7, 5, image::Rgba([10, 120, 250, 255]));
        let out = scale(&img, 4);
        assert!(out.pixels().all(|p| p.0 == [10, 120, 250, 255]));
    }

    #[test]
    fn tiny_images_do_not_panic() {
        for (w, h) in [(1, 1), (1, 4), (4, 1), (2, 2)] {
            let img = RgbaImage::from_fn(w, h, |x, y| {
                image::Rgba([(x * 90) as u8, (y * 90) as u8, 0, 255])
            });
            assert_eq!(scale(&img, 4).dimensions(), (w * 4, h * 4));
        }
    }
}
