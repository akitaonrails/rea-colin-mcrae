//! The unified in-race **camera view controller** (`rea.official-eu.camera-modes`):
//! the per-frame camera driver / slot evaluator (`FUN_004343a0`/`FUN_00436490`)
//! that holds the active [`CameraMode`], advances it on the player's "next view"
//! input ([`camera_mode::cycle_next_view`]), and dispatches each frame to the
//! matching reconstructed placement to produce the camera world matrix.
//!
//! This ties the camera modules together into one driver the windowed session
//! can hold: chase ([`gameplay_camera`]) with its orientation ease, cockpit/mirror
//! ([`camera_interior`]), and the cinematic replay/TV cams ([`camera_cinematic`]).
//! The helmet look overlay, the trackside director, and the inter-mode crossfade
//! are their own (recovered) modules and are not folded in here; modes without a
//! self-contained placement fall back to the chase camera.

use crate::camera_cinematic::{cinematic_camera, cinematic_offset_fixed, cinematic_offset_random};
use crate::camera_interior::cockpit_camera;
use crate::camera_mode::{cycle_next_view, CameraMode, ViewGate};
use crate::fixed_point::{build_sine_table, Vec3, TRIG_TABLE_LEN};
use crate::game_rng::GameLcg;
use crate::gameplay_camera::{car_world_q16, ChasePreset, GameplayCamera};

/// Q16.16 one.
const ONE: i32 = 0x1_0000;

/// The stateful camera driver: the active mode plus the state the modes need (the
/// eased chase camera and the current cinematic shot offset).
pub struct ViewController {
    mode: CameraMode,
    chase: GameplayCamera,
    cinematic_offset: [i32; 3],
    trig: [i32; TRIG_TABLE_LEN],
}

impl ViewController {
    /// A controller starting in `mode` (the game seeds [`CameraMode::ChaseNear`]).
    #[must_use]
    pub fn new(mode: CameraMode) -> Self {
        Self {
            mode,
            chase: GameplayCamera::new(),
            cinematic_offset: cinematic_offset_fixed(),
            trig: build_sine_table(),
        }
    }

    /// The active camera mode.
    #[must_use]
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// Advance to the next selectable view (the player's "change camera" input):
    /// [`cycle_next_view`] under `gate`, re-seeding the cinematic shot offset when
    /// the new mode is a cinematic cam (fixed for the bonnet TV cam, a fresh
    /// random framing drawn from `rng` for the replay action cam).
    pub fn cycle(&mut self, gate: ViewGate, rng: &mut GameLcg) {
        self.mode = cycle_next_view(self.mode, gate);
        match self.mode {
            CameraMode::Bonnet => self.cinematic_offset = cinematic_offset_fixed(),
            CameraMode::ReplayAction => self.cinematic_offset = cinematic_offset_random(rng),
            _ => {}
        }
    }

    /// The camera world matrix for this frame, dispatched by the active mode from
    /// the car's sim pose (`FUN_00436490`). Chase modes ease toward the target;
    /// cockpit/mirror and the cinematic cams are placed directly; any mode without
    /// a self-contained placement here falls back to the near chase.
    pub fn update(&mut self, position: Vec3, heading: u16, pal: bool) -> [i32; 16] {
        match self.mode {
            CameraMode::ChaseNear | CameraMode::ChaseFar | CameraMode::ChaseThird => {
                let preset = self.mode.chase_preset().unwrap_or(ChasePreset::Near);
                *self.chase.update(position, heading, preset, pal)
            }
            CameraMode::CockpitA => {
                let car = car_world_q16(&self.trig, position, heading);
                cockpit_camera(&car, 0, false, &self.trig)
            }
            CameraMode::CockpitB => {
                let car = car_world_q16(&self.trig, position, heading);
                cockpit_camera(&car, 0, true, &self.trig)
            }
            CameraMode::Bonnet | CameraMode::ReplayAction => {
                let car = car_world_q16(&self.trig, position, heading);
                let car_pos = [car[12], car[13], car[14]];
                // Bonnet looks along the car forward; the replay cam keeps world up.
                let up_ref = if self.mode == CameraMode::Bonnet {
                    [car[8], car[9], car[10]]
                } else {
                    [0, ONE, 0]
                };
                cinematic_camera(car_pos, self.cinematic_offset, up_ref)
            }
            _ => {
                // Helmet / trackside / showroom / sentinels: no self-contained
                // placement here - fall back to the near chase.
                *self.chase.update(position, heading, ChasePreset::Near, pal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::Fixed16;

    fn v(x: i32, y: i32, z: i32) -> Vec3 {
        Vec3::new(Fixed16::from_raw(x), Fixed16::from_raw(y), Fixed16::from_raw(z))
    }

    fn translation(m: &[i32; 16]) -> [i32; 3] {
        [m[12], m[13], m[14]]
    }

    #[test]
    fn starts_in_the_seeded_mode_and_cycles() {
        let mut vc = ViewController::new(CameraMode::ChaseNear);
        assert_eq!(vc.mode(), CameraMode::ChaseNear);
        let mut rng = GameLcg::new(1);
        // Cockpit enabled, showroom blocked: near -> far -> cockpit A ...
        let gate = ViewGate { cockpit_enabled: true, showroom_allowed: false };
        vc.cycle(gate, &mut rng);
        assert_eq!(vc.mode(), CameraMode::ChaseFar);
    }

    #[test]
    fn chase_mode_places_the_camera_behind_and_above_a_still_car() {
        let mut vc = ViewController::new(CameraMode::ChaseNear);
        let m = vc.update(v(0, 0, 0), 0, false);
        // Near chase sits above the ground and behind the origin car.
        assert!(m[13] > 0, "camera above ground");
        assert_ne!(translation(&m), [0, 0, 0]);
    }

    #[test]
    fn cockpit_mode_places_the_eye_near_the_car() {
        let mut vc = ViewController::new(CameraMode::CockpitA);
        let m = vc.update(v(0, 0, 0), 0, false);
        // The cockpit eye is roughly at driver height (~1.0), close to the car.
        assert!(m[13] > 0 && m[13] < 0x2_0000, "eye near ~1.0 height, got {}", m[13]);
    }

    #[test]
    fn entering_the_replay_cam_seeds_a_random_offset_within_envelope() {
        let mut vc = ViewController::new(CameraMode::ChaseNear);
        // Force the mode to the replay action cam via a direct cycle target: a gate
        // that leaves only chase won't reach it, so drive the offset seed directly.
        let mut rng = GameLcg::new(7);
        vc.mode = CameraMode::ReplayAction;
        vc.cinematic_offset = cinematic_offset_random(&mut rng);
        let m = vc.update(v(0, 0, 0), 0, false);
        // The eye is offset from the car by the cinematic distance (4.5..10).
        let off = translation(&m);
        let len = ((off[0] as f64 / 65536.0).powi(2)
            + (off[1] as f64 / 65536.0).powi(2)
            + (off[2] as f64 / 65536.0).powi(2))
        .sqrt();
        assert!((4.0..10.5).contains(&len), "replay cam distance {len} out of envelope");
    }
}
