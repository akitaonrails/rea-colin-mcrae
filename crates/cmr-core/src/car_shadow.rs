//! The in-race car shadow ("Cm" shadows): a textured, alpha-blended polygon
//! rebuilt every frame on the ground under each car and stretched away from
//! the stage's sun.
//!
//! - [`ShadowSun::from_shp`] is the stage load's sun setup `FUN_00449bd0`
//!   (azimuth and elevation of the stage's `.shp` record; the default without
//!   one) and [`ShadowSun::light`] the light direction `FUN_0047b000` derives.
//! - [`ShadowProfile::for_model`] is `FUN_0047b000`'s per-car silhouette
//!   (fourteen cases keyed by the car state's `+0x739` byte, code immediates),
//!   halved in the small-car mode (`cfg[0x25] & 4`).
//! - [`build_template`] is `FUN_0047c120`: an in-plane box from the eight
//!   contact points, shrunk toward the ground centre by height (`1 - 0.05 gap`,
//!   at least 0.4), then either kept as the simple four-point shadow or
//!   extruded along the sun into a seven-point hexagon, whose far edge an
//!   upright car bends with its profile (eleven points). Each point is snapped
//!   to the ground under it (`FUN_00446c40`, at most 0.5 from the plane).
//! - [`camera_pull`] is `FUN_0047bee0` (each point one unit toward the camera,
//!   two in split screen) and [`shadow_vertices`] the draw `FUN_004a51f0`'s
//!   pre-lit vertices, drawn with [`QUAD_FAN_INDICES_11`] /
//!   [`QUAD_FAN_INDICES_15`] after the track and before the objects, z-write
//!   off, alpha blended.
//!
//! A description of disassembly, not original source. Evidence:
//! `rea.official-eu.car-shadow`. Not reconstructed: the frozen-snapshot
//! fields (`DAT_00551afc`), the per-slot scale `DAT_00550d50` and the
//! special event's box shadows (`FUN_0047e900`).

use crate::legacy_math::sqrt_q16;

const ONE: i32 = 0x1_0000;

fn q(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

fn reciprocal(value: i32) -> i32 {
    if value == 0 {
        return 0;
    }
    (0x1_0000_0000_i64 / i64::from(value)) as i32
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [0, 1, 2].map(|i| a[i].wrapping_add(b[i]))
}

fn sub(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [0, 1, 2].map(|i| a[i].wrapping_sub(b[i]))
}

fn scale(v: [i32; 3], s: i32) -> [i32; 3] {
    v.map(|c| q(c, s))
}

fn dot(a: [i32; 3], b: [i32; 3]) -> i32 {
    q(a[0], b[0])
        .wrapping_add(q(a[1], b[1]))
        .wrapping_add(q(a[2], b[2]))
}

fn cross(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [
        q(a[1], b[2]).wrapping_sub(q(a[2], b[1])),
        q(a[2], b[0]).wrapping_sub(q(a[0], b[2])),
        q(a[0], b[1]).wrapping_sub(q(a[1], b[0])),
    ]
}

fn length(v: [i32; 3]) -> i32 {
    sqrt_q16(dot(v, v) as u32) as i32
}

fn normalize(v: [i32; 3]) -> [i32; 3] {
    let len = length(v);
    if len == 0 {
        return [0; 3];
    }
    scale(v, reciprocal(len))
}

/// The stage's sun (`DAT_0055aec0..c8`, `DAT_0055aebc`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowSun {
    /// The sun's position direction, scaled by 100.
    pub source: [i32; 3],
    /// The shadow length factor (Q16, 1.5 at 15 degrees or lower, 0 at 90).
    pub length: i32,
}

impl Default for ShadowSun {
    /// Without a `.shp` record: `(50, 75, 75)` and length 1.
    fn default() -> Self {
        Self {
            source: [0x32_0000, 0x4b_0000, 0x4b_0000],
            length: ONE,
        }
    }
}

impl ShadowSun {
    /// The `.shp` record's azimuth (`+0x1c`) and elevation (`+0x24`), `f64`
    /// radians, when it holds them.
    pub fn parse_shp(bytes: &[u8]) -> Option<(f64, f64)> {
        let f = |at: usize| {
            bytes
                .get(at..at + 8)
                .map(|b| f64::from_le_bytes(b.try_into().expect("eight bytes")))
        };
        Some((f(0x1c)?, f(0x24)?))
    }

    /// `FUN_00449bd0`: degrees from the record, the length factor from the
    /// elevation over 15..90 degrees, and the direction from azimuth and
    /// zenith, scaled by 100. The x87's extended intermediates are taken as
    /// `f64` (backend policy).
    pub fn from_shp(azimuth: f64, elevation: f64) -> Self {
        let pi = std::f64::consts::PI;
        let mut az = azimuth * (1.0 / pi) * 180.0;
        if az < 0.0 {
            az += 360.0;
        }
        let el = (elevation - pi) * -180.0 * (1.0 / pi);
        let t = ((el - 15.0) * (1.0 / 75.0)).clamp(0.0, 1.0);
        let length = q(((1.0 - t) * 65536.0) as i32, 0x1_8000);
        let a = az * pi / 180.0;
        let z = (90.0 - el) * pi / 180.0;
        let source = [
            (a.cos() * z.sin() * 65536.0) as i32,
            (z.cos() * 65536.0) as i32,
            (a.sin() * z.sin() * 65536.0) as i32,
        ]
        .map(|c| q(c, 0x64_0000));
        Self { source, length }
    }

    /// `FUN_0047b000`'s light direction `DAT_005ff990`: the source scaled by
    /// `-1 / max |component|`, normalized (it points down, the way the light
    /// travels).
    pub fn light(&self) -> [i32; 3] {
        let m = self
            .source
            .iter()
            .map(|c| c.wrapping_abs())
            .max()
            .unwrap_or(0);
        normalize(scale(self.source, reciprocal(m.wrapping_neg())))
    }
}

/// One car's shadow profile (`DAT_005feff8 + 12 s`, `DAT_005ff7fc`,
/// `DAT_005ff99c`, `DAT_005ff84c/850`): the four silhouette points in the
/// profile frame, the two extrusion scales and the shift limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowProfile {
    /// `(x, z)` of the four points.
    pub points: [[i32; 2]; 4],
    pub near_scale: i32,
    pub far_scale: i32,
    /// `DAT_005ff84c`, `DAT_005ff850`.
    pub shift_max: i32,
    pub shift_min: i32,
}

/// `FUN_0047b000`'s per-model constants: the four points, the two scales and
/// the span ends `A`, `B` (all `(x, z)`, Q16).
/// One model's row: the four points, the near and far scales, `A` and `B`.
type ProfileRow = ([[i32; 2]; 4], i32, i32, [i32; 2], [i32; 2]);

const PROFILE_TABLE: [ProfileRow; 14] = {
    const fn p(x: u32, z: u32) -> [i32; 2] {
        [x as i32, z as i32]
    }
    [
        (
            [
                p(0xdced, 0x47ef),
                p(0x43d7, 0xa666),
                p(0xffff_1c29, 0xad91),
                p(0xfffe_8f5d, 0x47ef),
            ],
            0xb0a3,
            0xfa9f,
            p(0x2_07ef, 0x147a),
            p(0xfffd_fd71, 0x4c08),
        ),
        (
            [
                p(0xef5c, 0x472b),
                p(0x1b64, 0xbb22),
                p(0xfffe_aeda, 0xb2b0),
                p(0xfffe_343a, 0x53b6),
            ],
            0xc24d,
            0x1_0a7e,
            p(0x1_d439, 0x0b85),
            p(0xfffe_343a, 0x53b6),
        ),
        (
            [
                p(0xf893, 0x4312),
                p(0x4666, 0xaac0),
                p(0xfffe_d99a, 0xa3d7),
                p(0xfffe_3b23, 0x4041),
            ],
            0x6d91,
            0xa872,
            p(0x1_d1eb, 0xffff_f74c),
            p(0xfffe_3b23, 0x4041),
        ),
        (
            [
                p(0xa76c, 0x476c),
                p(0x072b, 0xa2d0),
                p(0xfffe_e6ea, 0xa106),
                p(0xfffe_6c09, 0x6ed9),
            ],
            0x9c28,
            0x1_16c8,
            p(0x1_e76c, 0x0189),
            p(0xfffd_f021, 0xa8b4),
        ),
        (
            [
                p(0xc000, 0x3999),
                p(0xffff_d4bd, 0x9020),
                p(0xffff_1582, 0x9c6a),
                p(0xfffe_56c9, 0x4ac0),
            ],
            0x8f1a,
            0xdb64,
            p(0x1_cb85, 0x1687),
            p(0xfffe_049c, 0x62d0),
        ),
        (
            [
                p(0x9df3, 0x38d4),
                p(0x34bc, 0x9df3),
                p(0xffff_2419, 0x926e),
                p(0xfffe_9b23, 0x4312),
            ],
            0xc6e9,
            0xd126,
            p(0x1_ec49, 0x1d70),
            p(0xfffe_10a4, 0x27ae),
        ),
        (
            [
                p(0x1_13f7, 0x4395),
                p(0x3a5e, 0xaf9d),
                p(0xffff_0832, 0xab02),
                p(0xfffe_7605, 0x574b),
            ],
            0xc45a,
            0x1_3e35,
            p(0x1_f26e, 0x0f9d),
            p(0xfffd_f605, 0x8978),
        ),
        (
            [
                p(0xfba5, 0x4a7e),
                p(0x31a9, 0xba1c),
                p(0xfffe_5e36, 0x9e35),
                p(0xfffe_17cf, 0x2c49),
            ],
            0x95c2,
            0xa5a1,
            p(0x1_d810, 0x1c6a),
            p(0xfffe_17cf, 0x2c49),
        ),
        (
            [
                p(0xdcac, 0x3333),
                p(0x4395, 0xa624),
                p(0xfffe_dcad, 0xad4f),
                p(0xfffe_2c8c, 0x4000),
            ],
            0xc3d7,
            0xee56,
            p(0x1_ebc6, 0x1581),
            p(0xfffe_2c8c, 0x4000),
        ),
        (
            [
                p(0xda1c, 0x4189),
                p(0x16c8, 0xb4fd),
                p(0xfffe_3a5f, 0xac08),
                p(0xfffe_2084, 0x424d),
            ],
            0xbd2f,
            0xf2f1,
            p(0x1_ec49, 0x0c8b),
            p(0xfffe_2084, 0x424d),
        ),
        (
            [
                p(0xffbe, 0x3ae1),
                p(0x451e, 0xa2d0),
                p(0xfffe_f540, 0xa9fb),
                p(0xfffe_5ba6, 0x6f9d),
            ],
            0x8e14,
            0xe978,
            p(0x1_e353, 0x0ac0),
            p(0xfffe_0a7f, 0x93b6),
        ),
        (
            [
                p(0xd78d, 0x4c49),
                p(0x1f7c, 0xb439),
                p(0xfffe_f4bd, 0xa624),
                p(0xfffe_5605, 0x645a),
            ],
            0xc189,
            0x1_25a1,
            p(0x1_ce56, 0x170a),
            p(0xfffe_1959, 0x7b22),
        ),
        (
            [
                p(0x1_8d4f, 0x4000),
                p(0xc8f5, 0xab43),
                p(0xffff_747b, 0xc7ae),
                p(0xfffe_71aa, 0x2f1a),
            ],
            0xa8f5,
            0xb7ce,
            p(0x1_c72b, 0xffff_f063),
            p(0xfffe_2e57, 0xffff_fe36),
        ),
        (
            [
                p(0xe041, 0x3b64),
                p(0x3062, 0x8ac0),
                p(0xffff_0fe0, 0x93b6),
                p(0xfffe_5fbf, 0x5d2f),
            ],
            0x60c4,
            0xe624,
            p(0x2_170a, 0xffff_f9dc),
            p(0xfffd_f894, 0x8b02),
        ),
    ]
};

impl ShadowProfile {
    /// `FUN_0047b000` for one car: the model's constants (an unknown model
    /// takes the default, case 0), halved in the small-car mode, then the
    /// points re-expressed in the frame of the span `A`-`B` centred on its
    /// middle, with the shift limits.
    pub fn for_model(model: u8, small_cars: bool) -> Self {
        let (mut points, mut near, mut far, mut a, mut b) = PROFILE_TABLE
            .get(usize::from(model))
            .copied()
            .unwrap_or(PROFILE_TABLE[0]);
        if small_cars {
            let half = |v: [i32; 2]| v.map(|c| q(c, 0x8000));
            points = points.map(half);
            near = q(near, 0x8000);
            far = q(far, 0x8000);
            a = half(a);
            b = half(b);
        }
        let d = [a[0].wrapping_sub(b[0]), a[1].wrapping_sub(b[1])];
        let len = sqrt_q16(q(d[0], d[0]).wrapping_add(q(d[1], d[1])) as u32) as i32;
        let inverse = reciprocal(len);
        let u = [q(d[0], inverse), q(d[1], inverse)];
        let half = q(len, 0x8000);
        let m = [
            b[0].wrapping_add(q(u[0], half)),
            b[1].wrapping_add(q(u[1], half)),
        ];
        let shift_max = q(u[0], a[0].wrapping_sub(points[0][0]))
            .wrapping_add(q(u[1], a[1].wrapping_sub(points[0][1])));
        let shift_min = q(u[0], b[0].wrapping_sub(points[3][0]))
            .wrapping_add(q(u[1], b[1].wrapping_sub(points[3][1])));
        let points = points.map(|p| {
            let r = [p[0].wrapping_sub(m[0]), p[1].wrapping_sub(m[1])];
            [
                q(u[0], r[0]).wrapping_add(q(u[1], r[1])),
                q(u[1].wrapping_neg(), r[0]).wrapping_add(q(u[0], r[1])),
            ]
        });
        Self {
            points,
            near_scale: near,
            far_scale: far,
            shift_max,
            shift_min,
        }
    }
}

/// The car state the template reads (live fields; `F == 0`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowCar {
    /// `+0x324`: the ground normal.
    pub normal: [i32; 3],
    /// `+0x258`: the forward row, and `+0x264` the up row (the fallback
    /// axis and the upright test).
    pub forward: [i32; 3],
    pub up: [i32; 3],
    /// `+0xc8`: the centre.
    pub centre: [i32; 3],
    /// `+0xfc`: the eight contact points.
    pub probes: [[i32; 3]; 8],
    /// `+0x2e4..`: each contact's ground height.
    pub ground_heights: [i32; 8],
    /// `+0x73a..`: which contacts touch, and `+0x743` the fade enable.
    pub touching: [u8; 8],
    pub fade: bool,
}

/// What forces the simple four-point shadow (`FUN_0047c120`'s gate).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShadowConditions {
    /// The stage lighting is night (`DAT_00592f00 == 8`).
    pub night: bool,
    /// `FUN_0048d2d0`: overcast or wet.
    pub overcast: bool,
    /// Split screen outside the rally school (also skips the ground snap).
    pub split_screen: bool,
    /// `DAT_00514478`: the special event.
    pub special_event: bool,
    /// `DAT_00700244[slot] == 2`.
    pub attach_mode_two: bool,
}

/// One shadow (`0x88` bytes): up to eleven world points and the shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShadowEntry {
    pub points: [[i32; 3]; 11],
    /// 0: four points, 1: seven, 2: eleven.
    pub kind: u8,
}

impl ShadowEntry {
    /// The points the shape uses.
    pub const fn point_count(&self) -> usize {
        match self.kind {
            0 => 4,
            1 => 7,
            _ => 11,
        }
    }
}

/// The ground under one shadow point: `(x, z)` in, the ground height out
/// (`FUN_00446c40` with the slot's triangle and height caches, which the
/// provider owns).
pub trait ShadowGround {
    fn height(&mut self, index: usize, point: [i32; 3]) -> i32;
}

/// One slot's ground caches (`DAT_005ff5ec[slot * 11 + k]`, the `.hpc`
/// triangle handle, and `DAT_005ff69c`, the last height, which is also the
/// fallback when no triangle is found); zeroed at the stage load.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShadowGroundCache {
    pub handles: [i16; 11],
    pub heights: [i32; 11],
}

/// The stage's ground (`FUN_00446c40`) for one slot's shadow points.
pub struct CompiledShadowGround<'c, P> {
    pub provider: P,
    pub cache: &'c mut ShadowGroundCache,
}

impl<P: crate::compiled_car_initializer::CompiledLaneGeometryProvider> ShadowGround
    for CompiledShadowGround<'_, P>
{
    fn height(&mut self, index: usize, point: [i32; 3]) -> i32 {
        let input = crate::compiled_car_initializer::CompiledLaneDispatchInput {
            source_vector: point,
            initial_handle: self.cache.handles[index],
            fallback: self.cache.heights[index],
        };
        let Ok(done) =
            crate::compiled_car_initializer::dispatch_compiled_lane(input, &mut self.provider)
        else {
            return self.cache.heights[index];
        };
        let writes = done.handle_writes;
        if writes.normalized_to_minus_one || writes.invalidated_after_search_miss {
            self.cache.handles[index] = -1;
        }
        if let Some(handle) = writes.searched_handle {
            self.cache.handles[index] = handle;
        }
        self.cache.heights[index] = done.lane_word;
        done.lane_word
    }
}

/// `FUN_0047c120`'s ground snap: each point moves toward the ground height
/// by at most 0.5.
fn ground_snap(entry: &mut ShadowEntry, ground: &mut dyn ShadowGround) {
    for k in 0..entry.point_count() {
        let y = ground.height(k, entry.points[k]);
        let mut dy = y.wrapping_sub(entry.points[k][1]);
        if dy.wrapping_abs() > 0x8000 {
            dy = if dy >= 1 { 0x8000 } else { -0x8000 };
        }
        entry.points[k][1] = entry.points[k][1].wrapping_add(dy);
    }
}

/// `FUN_0047c120`: the car's shadow for this frame.
pub fn build_template(
    car: &ShadowCar,
    profile: &ShadowProfile,
    sun: &ShadowSun,
    conditions: ShadowConditions,
    school: bool,
    ground: &mut dyn ShadowGround,
) -> ShadowEntry {
    let n = car.normal;
    // (a) The in-plane basis.
    let mut src = car.forward;
    let mut d = dot(src, n);
    if d.wrapping_abs() >= 0xfd71 {
        src = car.up;
        d = dot(src, n);
    }
    let u = normalize(sub(src, scale(n, d)));
    let w = cross(u, n);
    // (b) The extents of the eight contacts.
    let extent = |axis: [i32; 3]| {
        car.probes
            .iter()
            .map(|p| dot(axis, sub(*p, car.centre)))
            .fold(0, i32::max)
    };
    let (eu, ew) = (scale(u, extent(u)), scale(w, extent(w)));
    let mut corners = [
        add(eu, ew),
        sub(eu, ew),
        sub([0; 3], add(eu, ew)),
        sub(ew, eu),
    ];
    // (c) The ground centre: the centre projected onto the plane through
    // contact 1's ground point.
    let g1 = [car.probes[1][0], car.ground_heights[1], car.probes[1][2]];
    let centre = add(car.centre, scale(n, dot(n, sub(g1, car.centre))));
    // (d) The height fade.
    let h = if car.fade {
        let gap = (0..8)
            .filter(|&i| car.touching[i] == 1)
            .map(|i| {
                car.ground_heights[i]
                    .wrapping_sub(car.probes[i][1])
                    .wrapping_abs()
            })
            .fold(0x2710_0000, i32::min);
        (ONE - q(gap, 0xccc).min(ONE)).max(0x6666)
    } else {
        ONE
    };
    if h != ONE {
        corners = corners.map(|c| scale(c, h));
    }
    let corners = corners.map(|c| add(c, centre));
    let mut entry = ShadowEntry {
        points: [[0; 3]; 11],
        kind: 0,
    };
    // (e) The simple shadow.
    if conditions.night
        || conditions.overcast
        || (!school && conditions.split_screen)
        || conditions.special_event
        || conditions.attach_mode_two
        || sun.length == 0
    {
        entry.points[..4].copy_from_slice(&corners);
        if school || !conditions.split_screen {
            ground_snap(&mut entry, ground);
        }
        return entry;
    }
    // (f) The sun extrusion.
    let l = sun.light();
    let lp = sub(l, scale(n, dot(l, n)));
    let s = q(h, sun.length);
    let o1 = scale(lp, q(profile.near_scale, s));
    let o2 = scale(lp, q(profile.far_scale, s));
    let (du, dw) = (dot(u, o1), dot(w, o1));
    let [c0, c1, c2, c3] = corners;
    let (points, edge, flip): ([[i32; 3]; 7], (usize, usize), bool) = if du < 0 && dw <= 0 {
        let (x1, x2, x3) = (add(c1, o1), add(c2, o2), add(c3, o2));
        ([c0, c1, c2, c3, x1, x2, x3], (4, 5), true)
    } else if du <= 0 && dw > 0 {
        let (x1, x2, x3) = (add(c0, o1), add(c2, o2), add(c3, o2));
        ([c1, c2, c3, c0, x2, x3, x1], (5, 6), false)
    } else if dw > 0 {
        let (x1, x2, x3) = (add(c0, o1), add(c1, o1), add(c3, o2));
        ([c2, c3, c0, c1, x3, x1, x2], (4, 5), false)
    } else {
        let (x1, x2, x3) = (add(c0, o1), add(c1, o1), add(c2, o2));
        ([c3, c0, c1, c2, x1, x2, x3], (5, 6), true)
    };
    entry.points[..7].copy_from_slice(&points);
    let half = scale(sub(points[edge.0], points[edge.1]), 0x8000);
    let middle = add(points[edge.1], half);
    let len = length(half);
    let e = scale(
        half,
        reciprocal(if flip { len } else { len.wrapping_neg() }),
    );
    let f2 = normalize(cross(e, n));
    let up = dot(n, car.up);
    if up < 0x4ccd {
        entry.kind = 1;
        ground_snap(&mut entry, ground);
        return entry;
    }
    // (g) The far edge bent with the profile.
    let k = q(q(q(up.wrapping_sub(0x4ccc), 0x1_6dd2), h), sun.length);
    let (a, b) = (dot(l, u), dot(l, w));
    let root = sqrt_q16(q(a, a).wrapping_add(q(b, b)) as u32) as i32;
    let inverse = reciprocal(root);
    let (an, bn) = (q(a, inverse), q(b, inverse));
    let mut shift = q(q(len, 0x3333), an);
    shift = if an < 1 {
        shift.max(profile.shift_min)
    } else {
        shift.min(profile.shift_max)
    };
    let pr = profile.points;
    let bent: [[i32; 3]; 4] = std::array::from_fn(|i| {
        let mut x = pr[i][0];
        if i == 1 || i == 2 {
            let span = if an < 1 {
                pr[3][0].wrapping_sub(pr[2][0])
            } else {
                pr[0][0].wrapping_sub(pr[1][0])
            };
            x = x.wrapping_add(q(span, an.wrapping_abs()));
        }
        let x = q(x.wrapping_add(shift), h);
        let mut z = q(pr[i][1], k);
        z = z.wrapping_sub(q(z, an.wrapping_abs()));
        if flip {
            z = z.wrapping_neg();
        }
        add(add(middle, scale(e, x)), scale(f2, z))
    });
    let p = &mut entry.points;
    let (p5, p6) = (p[5], p[6]);
    if an < 0 {
        if bn < 1 {
            p[9] = p5;
            p[10] = p6;
            p[5..9].copy_from_slice(&bent);
        } else {
            p[10] = p6;
            p[6..10].copy_from_slice(&[bent[3], bent[2], bent[1], bent[0]]);
        }
    } else if bn < 1 {
        p[10] = p6;
        p[6..10].copy_from_slice(&bent);
    } else if an < 1 {
        p[10] = p6;
        p[6..10].copy_from_slice(&[bent[3], bent[2], bent[1], bent[0]]);
    } else {
        p[9] = p5;
        p[10] = p6;
        p[5..9].copy_from_slice(&[bent[3], bent[2], bent[1], bent[0]]);
    }
    entry.kind = 2;
    ground_snap(&mut entry, ground);
    entry
}

/// `FUN_0047bee0`: each point farther than 2 (3 for a split screen's simple
/// shadow) from the camera moves 1 (0.5) unit toward it.
pub fn camera_pull(entry: &ShadowEntry, camera: [i32; 3], split_screen: bool) -> ShadowEntry {
    let mut out = *entry;
    let n = out.point_count();
    let (k, minimum) = if split_screen && n == 4 {
        (0x8000, 0x3_0000)
    } else {
        (ONE, 0x2_0000)
    };
    for point in &mut out.points[..n] {
        let d = sub(camera, *point);
        let len = length(d);
        if len > minimum {
            let t = q(len, k);
            if t != 0 {
                *point = add(*point, scale(d, reciprocal(t)));
            }
        }
    }
    out
}

/// One pre-lit vertex (`D3DLVERTEX`): world position, colour and texture
/// coordinates (the specular is `0xff000000`, no fog).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowVertex {
    pub position: [f32; 3],
    pub color: u32,
    pub uv: [f32; 2],
}

/// The 11-vertex shape's triangles (`DAT_00b73420`).
pub const QUAD_FAN_INDICES_11: [u16; 24] = [
    0, 1, 3, 1, 2, 3, 5, 10, 4, 6, 10, 5, 7, 10, 6, 8, 10, 7, 9, 10, 8, 4, 10, 9,
];
/// The 15-vertex shape's triangles (`DAT_00b73890`).
pub const QUAD_FAN_INDICES_15: [u16; 36] = [
    0, 1, 3, 1, 2, 3, 5, 14, 4, 6, 14, 5, 7, 14, 6, 8, 14, 7, 9, 14, 8, 10, 14, 9, 11, 14, 10, 12,
    14, 11, 13, 14, 12, 4, 14, 13,
];

/// `FUN_004a51f0`'s vertices for one shadow: black at alpha 200 inside and
/// 170 on the rim with blending (`0xff000000` without); a four-point shadow
/// is a strip, the others the quad plus a fan around the rim's centre.
pub fn shadow_vertices(entry: &ShadowEntry, blending: bool) -> Vec<ShadowVertex> {
    let (inner, outer) = if blending {
        (0xc800_0000, 0xaa00_0000)
    } else {
        (0xff00_0000, 0xff00_0000)
    };
    let p = &entry.points;
    let v = |point: [i32; 3], color: u32, uv: [f32; 2]| ShadowVertex {
        position: point.map(|c| (f64::from(c) * 1.525_878_906_25e-5) as f32),
        color,
        uv,
    };
    if entry.kind == 0 {
        return vec![
            v(p[1], inner, [1.0, 0.0]),
            v(p[2], inner, [1.0, 1.0]),
            v(p[0], inner, [0.0, 0.0]),
            v(p[3], inner, [0.0, 1.0]),
        ];
    }
    let mut out = vec![
        v(p[0], inner, [0.0, 0.0]),
        v(p[1], inner, [1.0, 0.0]),
        v(p[2], inner, [1.0, 1.0]),
        v(p[3], inner, [0.0, 1.0]),
    ];
    let ring: &[usize] = if entry.kind == 1 {
        &[0, 1, 4, 5, 6, 3]
    } else {
        &[0, 1, 4, 5, 6, 7, 8, 9, 10, 3]
    };
    for (i, &k) in ring.iter().enumerate() {
        let tv = if i % 2 == 0 { 0.7 } else { 0.3 };
        out.push(v(p[k], outer, [0.0, tv]));
    }
    let centre = add(p[0], scale(sub(p[5], p[0]), 0x8000));
    out.push(v(centre, outer, [0.5, 0.5]));
    out
}

/// The index list a shape draws with (`None`: a four-vertex strip).
pub fn shadow_indices(entry: &ShadowEntry) -> Option<&'static [u16]> {
    match entry.kind {
        0 => None,
        1 => Some(&QUAD_FAN_INDICES_11),
        _ => Some(&QUAD_FAN_INDICES_15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Flat(i32);
    impl ShadowGround for Flat {
        fn height(&mut self, _: usize, _: [i32; 3]) -> i32 {
            self.0
        }
    }

    fn car() -> ShadowCar {
        let probes = [
            [0x2_0000, 0, 0x1_0000],
            [0x2_0000, 0, -0x1_0000],
            [-0x2_0000, 0, 0x1_0000],
            [-0x2_0000, 0, -0x1_0000],
            [0x2_0000, 0x8000, 0x1_0000],
            [0x2_0000, 0x8000, -0x1_0000],
            [-0x2_0000, 0x8000, 0x1_0000],
            [-0x2_0000, 0x8000, -0x1_0000],
        ];
        ShadowCar {
            normal: [0, ONE, 0],
            forward: [ONE, 0, 0],
            up: [0, ONE, 0],
            centre: [0, 0x4000, 0],
            probes,
            ground_heights: [0; 8],
            touching: [1; 8],
            fade: false,
        }
    }

    #[test]
    fn the_default_sun_points_down_and_away() {
        let sun = ShadowSun::default();
        let l = sun.light();
        assert!(l[0] < 0 && l[1] < 0 && l[2] < 0);
        assert!((length(l) - ONE).abs() < 64);
        // Overhead: no length; low sun: 1.5.
        let pi = std::f64::consts::PI;
        assert_eq!(ShadowSun::from_shp(0.0, pi / 2.0).length, 0);
        assert_eq!(
            ShadowSun::from_shp(0.0, pi - 10.0 * pi / 180.0).length,
            0x1_8000
        );
        assert_eq!(ShadowSun::parse_shp(&[0; 0x2b]), None);
    }

    #[test]
    fn a_night_shadow_is_the_box_on_the_ground() {
        let profile = ShadowProfile::for_model(0, false);
        let conditions = ShadowConditions {
            night: true,
            ..Default::default()
        };
        let entry = build_template(
            &car(),
            &profile,
            &ShadowSun::default(),
            conditions,
            false,
            &mut Flat(0),
        );
        assert_eq!(entry.kind, 0);
        // Corners at +-2 along x, +-1 along z (W = U x N is +z), on the
        // ground; the table square root rounds a little low.
        let near = |a: i32, b: i32| (a - b).abs() < 128;
        assert!(near(entry.points[0][0], 0x2_0000) && near(entry.points[0][2], 0x1_0000));
        assert!(entry.points[..4].iter().all(|p| p[1] == 0));
        let vertices = shadow_vertices(&entry, true);
        assert_eq!(vertices.len(), 4);
        assert_eq!(vertices[0].color, 0xc800_0000);
        assert_eq!(shadow_indices(&entry), None);
    }

    #[test]
    fn the_snap_moves_at_most_half_a_unit() {
        let profile = ShadowProfile::for_model(3, false);
        let conditions = ShadowConditions {
            night: true,
            ..Default::default()
        };
        let entry = build_template(
            &car(),
            &profile,
            &ShadowSun::default(),
            conditions,
            false,
            &mut Flat(0x3_0000),
        );
        assert!(entry.points[..4].iter().all(|p| p[1] == 0x8000));
    }

    #[test]
    fn a_sunlit_upright_car_casts_the_eleven_point_shadow() {
        let profile = ShadowProfile::for_model(0, false);
        let entry = build_template(
            &car(),
            &profile,
            &ShadowSun::default(),
            ShadowConditions::default(),
            false,
            &mut Flat(0),
        );
        assert_eq!(entry.kind, 2);
        let vertices = shadow_vertices(&entry, true);
        assert_eq!(vertices.len(), 15);
        assert_eq!(vertices[4].color, 0xaa00_0000);
        assert_eq!(shadow_indices(&entry).map(<[u16]>::len), Some(36));
        // The extrusion goes the light's way (negative x and z).
        let l = ShadowSun::default().light();
        let far = entry.points[4];
        assert!(dot(sub(far, [0, 0, 0]), [l[0], 0, l[2]]) > 0);
        // A tilted car keeps the seven-point hexagon.
        let mut tilted = car();
        tilted.up = [ONE, 0, 0];
        tilted.forward = [0, 0, ONE];
        let entry = build_template(
            &tilted,
            &profile,
            &ShadowSun::default(),
            ShadowConditions::default(),
            false,
            &mut Flat(0),
        );
        assert_eq!(entry.kind, 1);
        assert_eq!(shadow_vertices(&entry, false).len(), 11);
    }

    #[test]
    fn the_height_fade_shrinks_toward_the_ground_centre() {
        let profile = ShadowProfile::for_model(0, false);
        let mut high = car();
        high.fade = true;
        high.probes.iter_mut().for_each(|p| p[1] = 0xa_0000);
        let conditions = ShadowConditions {
            night: true,
            ..Default::default()
        };
        let entry = build_template(
            &high,
            &profile,
            &ShadowSun::default(),
            conditions,
            false,
            &mut Flat(0),
        );
        // Gap 10: 1 - 0.5 = 0.5.
        assert!((entry.points[0][0] - 0x1_0000).abs() < 128);
    }

    #[test]
    fn the_camera_pull_moves_one_unit() {
        let entry = ShadowEntry {
            points: [[0; 3]; 11],
            kind: 0,
        };
        let pulled = camera_pull(&entry, [0, 0, 0xa_0000], false);
        assert!((pulled.points[0][2] - 0x1_0000).abs() < 128);
        let near = camera_pull(&entry, [0, 0, 0x1_0000], false);
        assert_eq!(near.points[0], [0; 3], "within 2 units: unchanged");
    }

    #[test]
    fn the_small_car_mode_halves_the_profile() {
        let full = ShadowProfile::for_model(5, false);
        let half = ShadowProfile::for_model(5, true);
        assert_eq!(half.near_scale, full.near_scale / 2);
        assert_eq!(
            ShadowProfile::for_model(99, false),
            ShadowProfile::for_model(0, false)
        );
    }
}
