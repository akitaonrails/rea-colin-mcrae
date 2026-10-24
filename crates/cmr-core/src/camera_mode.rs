//! The in-race camera **mode state machine** (`rea.official-eu.camera-modes`):
//! the camera-mode value set, the user "next view" cycle, the saved-preference
//! restore, and the central mode-change gate's crossfade/lock/duration rules.
//!
//! The camera record is 0x60 bytes; its mode lives at `+4` (`DAT_00550494` in
//! the working bank, `DAT_00550654` in the A/B slot bank) and is written only by
//! the per-mode builder `FUN_004362a0`. The per-frame driver `FUN_004343a0`
//! reads it, the change gate `FUN_00434060` sets it (guarding the in-flight
//! blend), the cycle input `FUN_00434210` advances it, and the preference
//! restore `FUN_004360a0` maps a saved nibble back onto it.
//!
//! This module reconstructs the fully-recovered, backend-neutral decision logic.
//! The crossfade curve and pose blend ([`super::camera_crossfade`]), the
//! placement transients, and the trackside director are separate concerns; the
//! chase placement itself is [`super::gameplay_camera`].

use crate::gameplay_camera::ChasePreset;

/// A camera mode, the `+4` field of the camera record. Mode `4` is unused (the
/// value set the builders and evaluators switch on is `0..=0xc` minus `4`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraMode {
    /// `0`: the record before its first real mode is built (init only).
    Uninit,
    /// `1`: interior/cockpit A (`FUN_00401090(_, 1)`).
    CockpitA,
    /// `2`: cockpit B (`FUN_00401090(_, 0)`).
    CockpitB,
    /// `3`: showroom/menu camera (`FUN_00444770`; never crossfaded).
    Showroom,
    /// `5`: near chase (`FUN_00419e10(_, 0)` -> variant `0`).
    ChaseNear,
    /// `6`: far chase (`FUN_00419e10(_, 3)` -> variant `3`).
    ChaseFar,
    /// `7`: bonnet/hood (`FUN_0046bc80(_, 1)`).
    Bonnet,
    /// `8`: helmet/special (`FUN_00467630`).
    Helmet,
    /// `9`: replay action cam, RNG-framed (`FUN_0046bc80(_, 0)`).
    ReplayAction,
    /// `10` (`0xa`): third/high chase (`FUN_00419e10(_, 6)` -> variant `6`).
    ChaseThird,
    /// `11` (`0xb`): trackside/cinematic (`FUN_004864d0`).
    Trackside,
    /// `12` (`0xc`): the sentinel stored in `+4` while a crossfade runs - not a
    /// camera, a lock that `FUN_00434060` refuses to transition out of.
    Blending,
}

impl CameraMode {
    /// The raw `+4` value.
    #[must_use]
    pub const fn raw(self) -> u32 {
        match self {
            CameraMode::Uninit => 0,
            CameraMode::CockpitA => 1,
            CameraMode::CockpitB => 2,
            CameraMode::Showroom => 3,
            CameraMode::ChaseNear => 5,
            CameraMode::ChaseFar => 6,
            CameraMode::Bonnet => 7,
            CameraMode::Helmet => 8,
            CameraMode::ReplayAction => 9,
            CameraMode::ChaseThird => 10,
            CameraMode::Trackside => 11,
            CameraMode::Blending => 12,
        }
    }

    /// The mode for a raw `+4` value, or `None` for the unused `4` and any value
    /// above `0xc`.
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        Some(match value {
            0 => CameraMode::Uninit,
            1 => CameraMode::CockpitA,
            2 => CameraMode::CockpitB,
            3 => CameraMode::Showroom,
            5 => CameraMode::ChaseNear,
            6 => CameraMode::ChaseFar,
            7 => CameraMode::Bonnet,
            8 => CameraMode::Helmet,
            9 => CameraMode::ReplayAction,
            10 => CameraMode::ChaseThird,
            11 => CameraMode::Trackside,
            12 => CameraMode::Blending,
            _ => return None,
        })
    }

    /// The chase preset this mode drives, or `None` if it is not a chase camera
    /// (modes `5`/`6`/`10` -> variants `0`/`3`/`6`).
    #[must_use]
    pub const fn chase_preset(self) -> Option<ChasePreset> {
        match self {
            CameraMode::ChaseNear => Some(ChasePreset::Near),
            CameraMode::ChaseFar => Some(ChasePreset::Far),
            CameraMode::ChaseThird => Some(ChasePreset::Third),
            _ => None,
        }
    }
}

/// The saved per-car view preference (`car+0xb7` nibble) restored by
/// `FUN_004360a0`, as an index `0..=4` into the recovered mapping.
///
/// `0 -> ChaseNear(5)`, `1 -> ChaseFar(6)`, `2 -> CockpitB(2)`, `3 -> CockpitA(1)`,
/// `4 -> 3 - (DAT_00828e28 != 0)`: `CockpitB(2)` when the alt-showroom flag is
/// set, else `Showroom(3)`. Any other index falls back to the near chase.
#[must_use]
pub fn mode_from_pref_index(index: u8, alt_showroom: bool) -> CameraMode {
    match index {
        0 => CameraMode::ChaseNear,
        1 => CameraMode::ChaseFar,
        2 => CameraMode::CockpitB,
        3 => CameraMode::CockpitA,
        4 => {
            if alt_showroom {
                CameraMode::CockpitB
            } else {
                CameraMode::Showroom
            }
        }
        _ => CameraMode::ChaseNear,
    }
}

/// The context flags that gate which modes the "next view" cycle may land on
/// (`FUN_00434210`), from `DAT_00d3ca80`/`DAT_00828e28`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct ViewGate {
    /// `uVar3 = (DAT_00d3ca80[0x25] & 4) != 0`: the interior cameras (cockpit
    /// A/B) are selectable and the bonnet cam is not.
    pub cockpit_enabled: bool,
    /// `DAT_00828e28 == 0 && !cockpit_enabled && (DAT_00d3ca80[0x26] & 1) == 0`:
    /// the showroom camera is a selectable view.
    pub showroom_allowed: bool,
}

/// Whether the "next view" cycle may stop on `mode` under `gate` (`FUN_00434210`'s
/// allowed-set test). The near and far chase are always selectable (near is the
/// cycle's guaranteed default stop); the cockpit/bonnet/showroom modes are gated.
#[must_use]
pub fn view_mode_allowed(mode: CameraMode, gate: ViewGate) -> bool {
    match mode {
        CameraMode::ChaseNear | CameraMode::ChaseFar => true,
        CameraMode::CockpitA | CameraMode::CockpitB => gate.cockpit_enabled,
        CameraMode::Bonnet => !gate.cockpit_enabled,
        CameraMode::Showroom => gate.showroom_allowed,
        _ => false,
    }
}

/// The highest raw mode value the cycle scans before wrapping (`0xd -> 0`).
const CYCLE_WRAP: u32 = 0xd;

/// The user "next view" cycle (`FUN_00434210`): from the current mode, advance
/// the raw value (wrapping `0xd -> 0`) to the next mode `view_mode_allowed`
/// admits under `gate`. Helmet (`8`) does not cycle (the handler returns early).
/// The near chase is always allowed, so the scan always terminates on it at
/// worst.
#[must_use]
pub fn cycle_next_view(current: CameraMode, gate: ViewGate) -> CameraMode {
    if current == CameraMode::Helmet {
        return current;
    }
    let mut raw = current.raw();
    for _ in 0..CYCLE_WRAP {
        raw = if raw >= CYCLE_WRAP { 0 } else { raw + 1 };
        if let Some(mode) = CameraMode::from_raw(raw).filter(|m| view_mode_allowed(*m, gate)) {
            return mode;
        }
    }
    CameraMode::ChaseNear
}

/// The base crossfade length in frames before the slow-motion multiplier:
/// `0x19` (25) normally, `0x32` (50) for a transition into or out of trackside.
const FADE_BASE_FRAMES: i32 = 0x19;
const FADE_BASE_FRAMES_TRACKSIDE: i32 = 0x32;

/// The crossfade length in frames, exactly as `FUN_00434060` seeds the counter
/// (`DAT_00550558`) and duration (`DAT_00550640`): `base * ((DAT_00551af0 != 0) +
/// 1)`, i.e. `base` doubled while the single slow-motion flag is set. `base` is
/// `0x32` when either side is [`CameraMode::Trackside`], else `0x19`.
///
/// `slow_motion` is the one boolean the seed site reads (`DAT_00551af0`); its
/// exact identity (PAL, replay slow-mo, or both) is not resolved in the static
/// image. There is **one** doubling factor at this site, not an independent
/// PAL-and-replay product.
#[must_use]
pub fn crossfade_duration_frames(from: CameraMode, to: CameraMode, slow_motion: bool) -> i32 {
    let base = if from == CameraMode::Trackside || to == CameraMode::Trackside {
        FADE_BASE_FRAMES_TRACKSIDE
    } else {
        FADE_BASE_FRAMES
    };
    base * (i32::from(slow_motion) + 1)
}

/// What the mode-change gate does with a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionResult {
    /// The current mode is [`CameraMode::Blending`] (`0xc`): the gate refuses the
    /// request and keeps the blend running.
    Locked,
    /// Switch straight to `new_mode` with no crossfade (either none was asked, or
    /// the showroom mode forces an instant cut).
    Immediate { new_mode: CameraMode },
    /// Start a crossfade to `new_mode` over `frames`.
    Crossfade { new_mode: CameraMode, frames: i32 },
}

/// Resolve a mode-change request the way `FUN_00434060(player, new, crossfade, _)`
/// does: a request while [`CameraMode::Blending`] is [`TransitionResult::Locked`];
/// a crossfade is suppressed when either side is [`CameraMode::Showroom`];
/// otherwise the crossfade runs for [`crossfade_duration_frames`].
#[must_use]
pub fn resolve_transition(
    current: CameraMode,
    new_mode: CameraMode,
    crossfade_requested: bool,
    slow_motion: bool,
) -> TransitionResult {
    if current == CameraMode::Blending {
        return TransitionResult::Locked;
    }
    let fade =
        crossfade_requested && current != CameraMode::Showroom && new_mode != CameraMode::Showroom;
    if !fade {
        return TransitionResult::Immediate { new_mode };
    }
    TransitionResult::Crossfade {
        new_mode,
        frames: crossfade_duration_frames(current, new_mode, slow_motion),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_round_trips_and_skips_the_unused_value() {
        for raw in 0u32..=12 {
            match CameraMode::from_raw(raw) {
                Some(mode) => assert_eq!(mode.raw(), raw),
                None => assert_eq!(raw, 4, "only 4 is unused in 0..=12"),
            }
        }
        assert_eq!(CameraMode::from_raw(4), None);
        assert_eq!(CameraMode::from_raw(13), None);
        assert_eq!(CameraMode::from_raw(0xffff_ffff), None);
    }

    #[test]
    fn chase_modes_map_to_the_recovered_presets() {
        assert_eq!(
            CameraMode::ChaseNear.chase_preset(),
            Some(ChasePreset::Near)
        );
        assert_eq!(CameraMode::ChaseFar.chase_preset(), Some(ChasePreset::Far));
        assert_eq!(
            CameraMode::ChaseThird.chase_preset(),
            Some(ChasePreset::Third)
        );
        assert_eq!(CameraMode::Bonnet.chase_preset(), None);
        assert_eq!(CameraMode::Trackside.chase_preset(), None);
    }

    #[test]
    fn preference_index_maps_as_recovered() {
        assert_eq!(mode_from_pref_index(0, false), CameraMode::ChaseNear);
        assert_eq!(mode_from_pref_index(1, false), CameraMode::ChaseFar);
        assert_eq!(mode_from_pref_index(2, false), CameraMode::CockpitB);
        assert_eq!(mode_from_pref_index(3, false), CameraMode::CockpitA);
        // Index 4 = 3 - (alt flag): showroom when clear, cockpit B when set.
        assert_eq!(mode_from_pref_index(4, false), CameraMode::Showroom);
        assert_eq!(mode_from_pref_index(4, true), CameraMode::CockpitB);
        // Out of range falls back to the near chase.
        assert_eq!(mode_from_pref_index(9, false), CameraMode::ChaseNear);
    }

    #[test]
    fn view_cycle_respects_the_gate() {
        // Cockpit enabled, showroom blocked: bonnet is excluded, cockpits in.
        let cockpit = ViewGate {
            cockpit_enabled: true,
            showroom_allowed: false,
        };
        assert!(view_mode_allowed(CameraMode::CockpitA, cockpit));
        assert!(!view_mode_allowed(CameraMode::Bonnet, cockpit));
        // From near chase (5): 6 is always allowed, so the next stop is far.
        assert_eq!(
            cycle_next_view(CameraMode::ChaseNear, cockpit),
            CameraMode::ChaseFar
        );
        // From far (6): 7 bonnet excluded, 8/9/10/11/12/0 excluded, wrap to 1.
        assert_eq!(
            cycle_next_view(CameraMode::ChaseFar, cockpit),
            CameraMode::CockpitA
        );

        // Cockpit disabled, showroom blocked: bonnet allowed, cockpits out.
        let exterior = ViewGate {
            cockpit_enabled: false,
            showroom_allowed: false,
        };
        assert_eq!(
            cycle_next_view(CameraMode::ChaseFar, exterior),
            CameraMode::Bonnet
        );
        // From bonnet (7): nothing allowed until the wrap lands on near (5).
        assert_eq!(
            cycle_next_view(CameraMode::Bonnet, exterior),
            CameraMode::ChaseNear
        );

        // Showroom allowed: it becomes a cycle stop.
        let showroom = ViewGate {
            cockpit_enabled: false,
            showroom_allowed: true,
        };
        assert_eq!(
            cycle_next_view(CameraMode::CockpitB, showroom),
            CameraMode::Showroom
        );

        // Helmet never cycles.
        assert_eq!(
            cycle_next_view(CameraMode::Helmet, cockpit),
            CameraMode::Helmet
        );
    }

    #[test]
    fn crossfade_duration_applies_slow_motion_and_trackside() {
        // Normal, chase-to-chase: 25 frames.
        assert_eq!(
            crossfade_duration_frames(CameraMode::ChaseNear, CameraMode::ChaseFar, false),
            25
        );
        // The single slow-motion flag doubles it (there is ONE factor, not a
        // PAL-and-replay product).
        assert_eq!(
            crossfade_duration_frames(CameraMode::ChaseNear, CameraMode::ChaseFar, true),
            50
        );
        // Into trackside uses the longer 0x32 base.
        assert_eq!(
            crossfade_duration_frames(CameraMode::ChaseNear, CameraMode::Trackside, false),
            50
        );
        // Out of trackside too, and the slow-motion flag still only doubles once.
        assert_eq!(
            crossfade_duration_frames(CameraMode::Trackside, CameraMode::ChaseNear, false),
            50
        );
        assert_eq!(
            crossfade_duration_frames(CameraMode::Trackside, CameraMode::ChaseNear, true),
            100
        );
    }

    #[test]
    fn transition_gate_locks_cuts_and_fades() {
        // Blending is locked.
        assert_eq!(
            resolve_transition(CameraMode::Blending, CameraMode::ChaseFar, true, false),
            TransitionResult::Locked
        );
        // No crossfade requested -> instant cut.
        assert_eq!(
            resolve_transition(CameraMode::ChaseNear, CameraMode::ChaseFar, false, false),
            TransitionResult::Immediate {
                new_mode: CameraMode::ChaseFar
            }
        );
        // Showroom on either side forces an instant cut despite the request.
        assert_eq!(
            resolve_transition(CameraMode::Showroom, CameraMode::ChaseFar, true, false),
            TransitionResult::Immediate {
                new_mode: CameraMode::ChaseFar
            }
        );
        assert_eq!(
            resolve_transition(CameraMode::ChaseNear, CameraMode::Showroom, true, false),
            TransitionResult::Immediate {
                new_mode: CameraMode::Showroom
            }
        );
        // Otherwise a crossfade over the recovered duration (slow-motion off = 25).
        assert_eq!(
            resolve_transition(CameraMode::ChaseNear, CameraMode::ChaseFar, true, false),
            TransitionResult::Crossfade {
                new_mode: CameraMode::ChaseFar,
                frames: 25
            }
        );
    }
}
