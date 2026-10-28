//! The front end's camera and row-swing geometry, over the executable's
//! static block `0x507a78..0x507ba0` (from the reviewed static tables).
//!
//! Every screen's create call (`FUN_004ae5f0`, argument 4, stored at menu
//! `+0x45c`) points at a two-dword camera target in that block; the camera
//! `DAT_004f1300/4` moves to it over a screen change (`FUN_0044f600`): x eases
//! quadratically on the ease value, y linearly. Camera-relative drawing adds
//! `OFF = (0x8000 - cam_x, 0x8000 - cam_y)` (`FUN_0044f7e0`). The boot camera
//! equals the root's target. The setup rows swing on the arc record
//! `0x507b88` (`R0`, `R1`, tilt): `FUN_004c4300`. A description of
//! disassembly, not original source. Evidence:
//! `rea.official-eu.frontend-camera`.

use crate::fixed_point::{TRIG_TABLE_LEN, build_sine_table};
use crate::frontend::{
    CHAMPIONSHIP_NEXT_SCREEN_ID, CHAMPIONSHIP_SETUP_SCREEN_ID, LOAD_SCREEN_ID,
    RALLY_NAME_SCREEN_ID, RALLY_SCREEN_ID, ROOT_SCREEN_ID, STAGE_SELECT_SCREEN_ID, ScreenId,
    TIME_TRIAL_NAME_SCREEN_ID, TIME_TRIAL_SCREEN_ID,
};

/// The block's base address.
pub const BLOCK_BASE: u32 = 0x0050_7a78;
/// The block's length.
pub const BLOCK_LEN: usize = 0x128;

/// Each reconstructed screen's camera target address.
const TARGETS: [(ScreenId, u32); 9] = [
    (ROOT_SCREEN_ID, 0x0050_7a78),
    (TIME_TRIAL_SCREEN_ID, 0x0050_7a88),
    (RALLY_SCREEN_ID, 0x0050_7a90),
    (CHAMPIONSHIP_SETUP_SCREEN_ID, 0x0050_7a98),
    (LOAD_SCREEN_ID, 0x0050_7aa0),
    (TIME_TRIAL_NAME_SCREEN_ID, 0x0050_7af0),
    (RALLY_NAME_SCREEN_ID, 0x0050_7af8),
    (CHAMPIONSHIP_NEXT_SCREEN_ID, 0x0050_7b00),
    (STAGE_SELECT_SCREEN_ID, 0x0050_7b08),
];

/// The arc record `0x507b88`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Arc {
    pub r0: i32,
    pub r1: i32,
    pub tilt: u16,
}

/// The parsed camera block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraBlock {
    bytes: Vec<u8>,
}

impl CameraBlock {
    /// The block, when it has the full length.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        (bytes.len() >= BLOCK_LEN).then(|| Self {
            bytes: bytes[..BLOCK_LEN].to_vec(),
        })
    }

    fn i32_at(&self, address: u32) -> Option<i32> {
        let at = usize::try_from(address.checked_sub(BLOCK_BASE)?).ok()?;
        let bytes = self.bytes.get(at..at + 4)?;
        Some(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// The camera target of `screen`, or `None` (a null target: no move).
    pub fn target(&self, screen: ScreenId) -> Option<(i32, i32)> {
        let address = TARGETS
            .iter()
            .find(|(s, _)| *s == screen)
            .map(|(_, address)| *address)
            .or_else(|| {
                crate::frontend_options::OptionScreen::of(screen).map(|o| o.camera_target())
            })
            .or_else(|| {
                crate::frontend_controls::ControlScreen::of(screen).map(|c| c.camera_target())
            })?;
        Some((self.i32_at(address)?, self.i32_at(address + 4)?))
    }

    /// The arc record.
    pub fn arc(&self) -> Arc {
        Arc {
            r0: self.i32_at(0x0050_7b88).unwrap_or(0),
            r1: self.i32_at(0x0050_7b8c).unwrap_or(0),
            tilt: self.i32_at(0x0050_7b90).unwrap_or(0) as u16,
        }
    }
}

/// `FUN_0048fba0`'s tangent table entry `DAT_00833780[index]`:
/// `ftol(tan(index * 2pi/4096) * 65536)`, with entries `0x400` and `0xc00`
/// then set to `0x7fffffff`. The x87 `fptan` is evaluated here in `f64`
/// (backend policy: the truncated values agree unless a product lands within
/// rounding of an integer).
pub fn tan_q16(index: u16) -> i32 {
    let index = index & 0xfff;
    if index == 0x400 || index == 0xc00 {
        return 0x7fff_ffff;
    }
    let value = (f64::from(index) * (std::f64::consts::TAU / 4096.0)).tan() * 65536.0;
    if value.is_finite() && value.abs() < 2_147_483_648.0 {
        value as i32
    } else {
        i32::MIN
    }
}

fn mul_q16(a: i32, b: i32) -> i32 {
    ((i64::from(a) * i64::from(b)) >> 16) as i32
}

/// `FUN_004c4300(arc, angle)`: a row's Q16 screen position on the arc.
pub fn arc_position(
    arc: Arc,
    angle: i32,
    sine: &[i32; TRIG_TABLE_LEN],
    width: i32,
    height: i32,
) -> (i32, i32) {
    let sin = sine[(angle & 0xfff) as usize];
    let cos = sine[(angle.wrapping_add(0x400) & 0xfff) as usize];
    let a = mul_q16(arc.r0, sin);
    let u = mul_q16(arc.r1, cos);
    let t = mul_q16(tan_q16(arc.tilt), u);
    let x = a
        .wrapping_sub(t)
        .wrapping_div(width.max(1))
        .wrapping_add(0x8000);
    let y = u.wrapping_div(height.max(1));
    (x, y)
}

/// The shared sine table (`DAT_0082d380`).
pub fn sine_table() -> [i32; TRIG_TABLE_LEN] {
    build_sine_table()
}

/// The camera position `DAT_004f1300/4` and its move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Camera {
    pub x: i32,
    pub y: i32,
    start: (i32, i32),
    delta: (i32, i32),
}

impl Camera {
    /// At `(x, y)`, still.
    pub const fn at(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            start: (x, y),
            delta: (0, 0),
        }
    }

    /// `FUN_0044f6d0`: move from here to `target` (no move without one).
    pub fn aim(&mut self, target: Option<(i32, i32)>) {
        self.start = (self.x, self.y);
        self.delta = target.map_or((0, 0), |(x, y)| {
            (x.wrapping_sub(self.x), y.wrapping_sub(self.y))
        });
    }

    /// `FUN_0044f600`'s placement for the ease value `t`.
    pub fn place(&mut self, t: i32) {
        let eased = mul_q16(t, t);
        self.x = self.start.0.wrapping_add(mul_q16(eased, self.delta.0));
        self.y = self.start.1.wrapping_add(mul_q16(t, self.delta.1));
    }

    /// `FUN_0044f7e0`: the camera-relative offset.
    pub const fn offset(&self) -> (i32, i32) {
        (0x8000 - self.x, 0x8000 - self.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block() -> CameraBlock {
        let mut bytes = vec![0u8; BLOCK_LEN];
        let mut put = |address: u32, value: i32| {
            let at = (address - BLOCK_BASE) as usize;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        };
        put(0x0050_7a78, 0x8000);
        put(0x0050_7a98, 0x18000);
        put(0x0050_7a9c, -0x8000);
        put(0x0050_7b88, 200 << 16);
        put(0x0050_7b8c, 100 << 16);
        put(0x0050_7b90, 0);
        CameraBlock::parse(&bytes).unwrap()
    }

    #[test]
    fn targets_and_the_arc() {
        let block = block();
        assert_eq!(block.target(ROOT_SCREEN_ID), Some((0x8000, 0)));
        assert_eq!(
            block.target(CHAMPIONSHIP_SETUP_SCREEN_ID),
            Some((0x18000, -0x8000))
        );
        assert_eq!(block.arc().r0, 200 << 16);
        assert!(CameraBlock::parse(&[0; 4]).is_none());
    }

    #[test]
    fn the_camera_eases_x_quadratically_and_y_linearly() {
        let mut camera = Camera::at(0x8000, 0);
        camera.aim(Some((0x18000, -0x10000)));
        camera.place(0x8000);
        assert_eq!(camera.x, 0x8000 + 0x10000 / 4);
        assert_eq!(camera.y, -0x8000);
        camera.place(0x10000);
        assert_eq!((camera.x, camera.y), (0x18000, -0x10000));
        assert_eq!(camera.offset(), (0x8000 - 0x18000, 0x8000 + 0x10000));
        camera.aim(None);
        camera.place(0x8000);
        assert_eq!(
            (camera.x, camera.y),
            (0x18000, -0x10000),
            "no target: no move"
        );
    }

    #[test]
    fn the_tangent_table_and_the_arc_position() {
        assert_eq!(tan_q16(0), 0);
        assert_eq!(
            tan_q16(0x200),
            0x10000 - 1,
            "tan 45 degrees truncates below 1.0"
        );
        assert_eq!(tan_q16(0x400), 0x7fff_ffff);
        let sine = sine_table();
        // Angle 0: x at the centre, y = R1 / H.
        let (x, y) = arc_position(block().arc(), 0, &sine, 640, 480);
        assert_eq!(x, 0x8000);
        assert_eq!(y, (100 << 16) / 480);
        // A quarter turn swings the row out by R0 / W.
        let (x, y) = arc_position(block().arc(), 0x400, &sine, 640, 480);
        assert_eq!(x, (200 << 16) / 640 + 0x8000);
        assert_eq!(y, 0);
    }
}
