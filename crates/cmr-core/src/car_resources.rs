//! Car model / livery / wheel / damage resource composition (item E09).
//!
//! # Provenance
//!
//! A car is not a single bundled file: it is a set of sibling resources sharing
//! a base name under `OBJECTS\CARS\`, mirroring the stage design (E08). The
//! per-car loader `FUN_00480e60` (driven in a loop by race setup `FUN_00481350`,
//! torn down via a "Car Data Destructor" registration) composes filenames at
//! load time and loads each through the **same CMPR-capable scene loader
//! `FUN_00495ed0`** used for stages. The on-disk car format is therefore the
//! stage `.C3D` scene-graph format (parsed at the primitive level by
//! [`cmr-formats::c3d`](crate) / recovered in E02-E07), not a distinct car
//! container. This module reconstructs the car-specific *composition*: the model
//! selection, the path construction, the scene node-id contract and recursive
//! node search (`FUN_00495050`), the wheel radius formula, and the damage
//! addressing.
//!
//! The per-car set: a body model `OBJECTS\CARS\<code><DAY|NIGHT>.C3D`, a
//! reflection/shadow model `<code><tag>ref.c3d`, a shared reflection map
//! `ref_map.bmp`, plus low-detail variants. Wheels are not separate files - they
//! are four scene sub-nodes (ids 1-4) found inside the body model, with the
//! radius derived from node id 6. Damage uses no extra mesh files: real-time
//! vertex deformation (`FUN_0044b600`) plus per-region panel shedding
//! (`FUN_0044a770`, reusing the G04 `0x3333`/`0x6666` thresholds in
//! [`crate::collision`]).
//!
//! ## What is reconstructed vs. left as game data
//!
//! The 14-entry car-code table (`0x4f0d68` -> code strings at `0x4f0f04`) and
//! the full make/model display names (`0x508dc8`) are **game data and are not
//! vendored**, consistent with the G06 roster decision. The path builders take
//! the car code as a caller-supplied argument; only the path *structure* (the
//! `OBJECTS\CARS\` directory, the `DAY`/`NIGHT` tags, the `.C3D`/`ref.c3d`/
//! `ref_map.bmp`/`med`/`daymed` suffixes) is encoded, exactly as the E08 stage
//! extensions are. No payload bytes are reproduced.
//!
//! See evidence `rea.official-eu.car-resources`.

use crate::fixed_point::Vec3;

/// Number of selectable car models (`0x4f0d68` pointer table).
pub const CAR_MODEL_COUNT: usize = 14;

/// Per-player stride within the global settings block `DAT_00d3ca80`.
pub const PER_PLAYER_SETTINGS_STRIDE: usize = 0x9c;

/// Offset of the car-model selection byte within one player's settings
/// (`DAT_00d3ca80 + 0xb6 + player*0x9c`; the model index is its low nibble).
pub const CAR_MODEL_CONFIG_OFFSET: usize = 0xb6;

/// Car state block stride (`DAT_00700300 + player*0x768`).
pub const CAR_STATE_STRIDE: usize = 0x768;

/// Offset of the car-type byte within a car state block (selects the per-type
/// param region; see [`car_type_param_base`]).
pub const CAR_TYPE_STATE_OFFSET: usize = 0x738;

/// Offset of the damage-variant byte within a car state block (selects the
/// damage-region layout).
pub const DAMAGE_VARIANT_STATE_OFFSET: usize = 0x739;

/// Directory prefix all in-race car resources are built under.
pub const CAR_DIR: &str = r"OBJECTS\CARS\";
/// Body-model extension.
pub const CAR_MODEL_EXT: &str = ".C3D";
/// Reflection/shadow-model suffix (appended to `<code><tag>`).
pub const REFLECTION_SUFFIX: &str = "ref.c3d";
/// Shared reflection environment-map file name.
pub const REFLECTION_MAP: &str = "ref_map.bmp";
/// Low-detail suffix used for the night model.
pub const MED_SUFFIX: &str = "med";
/// Low-detail suffix used for the day model.
pub const DAYMED_SUFFIX: &str = "daymed";
/// Daytime model tag.
pub const DAY_TAG: &str = "DAY";
/// Night model tag (selected when the recovered night-mode predicate holds).
pub const NIGHT_TAG: &str = "NIGHT";

/// Keyed C3D loader mode used by one in-race body branch and the reflection.
pub const IN_RACE_KEYED_LOAD_MODE: i32 = 5;
/// Unkeyed C3D loader mode used by the other in-race body branch.
pub const IN_RACE_UNKEYED_LOAD_MODE: i32 = 6;
/// The fourth `FUN_004a2b70` argument used by both in-race actors.
pub const IN_RACE_CALLER_OPTION: i32 = 1;
/// Daytime/detail flags passed to both in-race actor loads.
pub const IN_RACE_DAY_LOAD_FLAGS: u8 = 8;
/// Night-time/detail flags passed to both in-race actor loads.
pub const IN_RACE_NIGHT_LOAD_FLAGS: u8 = 2;

/// The observed body-loader branch selected by `FUN_00480e60`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InRaceBodyLoadBranch {
    /// Mode 5, followed by `FUN_0045bb70(actor, 0xff)`.
    KeyedMode5,
    /// Mode 6, without the actor-alpha call.
    UnkeyedMode6,
}

/// Neutral inputs to the body-mode predicate at `0x48106c..0x48108e`.
///
/// The field names retain addresses because the runtime meanings of these
/// globals and queries are not yet established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InRaceBodyLoadContext {
    pub global_d3ca88_nonzero: bool,
    pub query_485bb0_nonzero: bool,
    pub query_401060_nonzero: bool,
    pub global_514478_nonzero: bool,
}

/// Resolves the exact observed body-mode branch from its neutral inputs.
#[must_use]
pub const fn in_race_body_load_branch(context: InRaceBodyLoadContext) -> InRaceBodyLoadBranch {
    if !context.global_d3ca88_nonzero
        && (context.query_485bb0_nonzero
            || context.query_401060_nonzero
            || context.global_514478_nonzero)
    {
        InRaceBodyLoadBranch::KeyedMode5
    } else {
        InRaceBodyLoadBranch::UnkeyedMode6
    }
}

/// Resolved loader policy for an in-race body/reflection pair.
///
/// The body conditionally uses mode 5 or 6. Mode 5 installs a zero
/// `DDCKEY_SRCBLT` color key, while mode 6 does not. The separate reflection
/// resource always uses mode 5 and is therefore also zero-keyed. Both observed
/// flag values have bit zero clear, so
/// [`crate::c3d_mesh::node_mesh_vertex_type`] selects lit LVERTEX geometry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InRaceCarLoadPolicy {
    pub body_mode: i32,
    pub reflection_mode: i32,
    pub caller_option: i32,
    pub load_flags: u8,
    pub body_source_color_key: Option<u8>,
    pub reflection_source_color_key: Option<u8>,
    pub body_sets_actor_alpha_ff: bool,
}

/// Returns the observed in-race loader policy for a body branch and day/night.
#[must_use]
pub const fn in_race_car_load_policy(
    body_branch: InRaceBodyLoadBranch,
    night: bool,
) -> InRaceCarLoadPolicy {
    let (body_mode, body_source_color_key, body_sets_actor_alpha_ff) = match body_branch {
        InRaceBodyLoadBranch::KeyedMode5 => (IN_RACE_KEYED_LOAD_MODE, Some(0), true),
        InRaceBodyLoadBranch::UnkeyedMode6 => (IN_RACE_UNKEYED_LOAD_MODE, None, false),
    };
    InRaceCarLoadPolicy {
        body_mode,
        reflection_mode: IN_RACE_KEYED_LOAD_MODE,
        caller_option: IN_RACE_CALLER_OPTION,
        load_flags: if night {
            IN_RACE_NIGHT_LOAD_FLAGS
        } else {
            IN_RACE_DAY_LOAD_FLAGS
        },
        body_source_color_key,
        reflection_source_color_key: Some(0),
        body_sets_actor_alpha_ff,
    }
}

/// Byte offset of a player's settings within `DAT_00d3ca80`.
pub fn player_settings_offset(player: usize) -> usize {
    player * PER_PLAYER_SETTINGS_STRIDE
}

/// Byte offset of a car state block within `DAT_00700300`.
pub fn car_state_offset(player: usize) -> usize {
    player * CAR_STATE_STRIDE
}

/// The selected car-model index: the low nibble of the settings byte
/// (`FUN_0044da00`). The nibble is `0..=15`; only `0..CAR_MODEL_COUNT` are valid
/// (see [`is_valid_car_model`]).
pub fn car_model_index(settings_byte: u8) -> u8 {
    settings_byte & 0x0f
}

/// Whether a model index is within the selectable range.
pub fn is_valid_car_model(index: u8) -> bool {
    (index as usize) < CAR_MODEL_COUNT
}

/// The day/night tag for a model name.
pub fn day_night_tag(night: bool) -> &'static str {
    if night { NIGHT_TAG } else { DAY_TAG }
}

/// Build the body-model path for a car code: `OBJECTS\CARS\<code><tag>.C3D`, or
/// the low-detail variant when `low_detail` is set. The low-detail pairing
/// (night -> `med`, day -> `daymed`) is inferred from the recovered suffix
/// strings; the `<tag>` is dropped in that case because the `med`/`daymed`
/// suffixes fold the day/night distinction.
pub fn car_body_path(code: &str, night: bool, low_detail: bool) -> String {
    if low_detail {
        let suffix = if night { MED_SUFFIX } else { DAYMED_SUFFIX };
        format!("{CAR_DIR}{code}{suffix}{CAR_MODEL_EXT}")
    } else {
        format!("{CAR_DIR}{code}{}{CAR_MODEL_EXT}", day_night_tag(night))
    }
}

/// Build the reflection/shadow-model path: `OBJECTS\CARS\<code><tag>ref.c3d`.
pub fn car_reflection_path(code: &str, night: bool) -> String {
    format!("{CAR_DIR}{code}{}{REFLECTION_SUFFIX}", day_night_tag(night))
}

/// The shared reflection environment-map path. The per-car base-name prefix is
/// empty in the shipped EU build, so the map resolves under the car directory.
pub fn car_reflection_map_path() -> String {
    format!("{CAR_DIR}{REFLECTION_MAP}")
}

/// Mask applied to a scene node's type/id word (`node[0xb] & 0xfff`).
pub const NODE_ID_MASK: u16 = 0xfff;

/// The four wheel scene-node ids inside the body model.
pub const WHEEL_NODE_IDS: [u16; 4] = [1, 2, 3, 4];

/// The scene-node id whose size field yields the wheel radius.
pub const WHEEL_RADIUS_NODE_ID: u16 = 6;

/// A portable view of one car scene-graph node: its masked type/id and its
/// child nodes. The shipped layout is a raw sibling/child pointer tree
/// (`node[0]` next sibling, `node[1]` first child, `node[0xb] & 0xfff` id); a
/// backend binds those pointers into this tree before the search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarSceneNode {
    /// Node type/id (already masked with [`NODE_ID_MASK`]).
    pub id: u16,
    /// Child nodes, in sibling order.
    pub children: Vec<CarSceneNode>,
}

impl CarSceneNode {
    /// Construct a node with an id and children.
    pub fn new(id: u16, children: Vec<CarSceneNode>) -> Self {
        Self { id, children }
    }

    /// A leaf node.
    pub fn leaf(id: u16) -> Self {
        Self {
            id,
            children: Vec::new(),
        }
    }
}

/// Find the first node with the given id, reproducing the recursive search
/// `FUN_00495050` (pre-order: check this node, then recurse its children in
/// sibling order). The id comparison is masked with [`NODE_ID_MASK`], matching
/// the recovered `node[0xb] & 0xfff` read.
pub fn find_node_by_id(root: &CarSceneNode, id: u16) -> Option<&CarSceneNode> {
    let target = id & NODE_ID_MASK;
    if root.id & NODE_ID_MASK == target {
        return Some(root);
    }
    for child in &root.children {
        if let Some(found) = find_node_by_id(child, id) {
            return Some(found);
        }
    }
    None
}

/// Resolve the four wheel nodes by id (`FUN_00495050(container, 1..4)`). An
/// absent wheel node is `None`.
pub fn find_wheels(root: &CarSceneNode) -> [Option<&CarSceneNode>; 4] {
    [
        find_node_by_id(root, WHEEL_NODE_IDS[0]),
        find_node_by_id(root, WHEEL_NODE_IDS[1]),
        find_node_by_id(root, WHEEL_NODE_IDS[2]),
        find_node_by_id(root, WHEEL_NODE_IDS[3]),
    ]
}

/// Derive the wheel radius from node id 6's fields, reproducing
/// `(size * scale >> 16) / 2` (Q16.16). When `rescale_half` is set (config
/// `DAT_00d3ca80 + 0x25 & 4`) the result is further scaled by `0x8000` (i.e.
/// halved again).
pub fn wheel_radius(size: i32, scale: i32, rescale_half: bool) -> i32 {
    let base = (((size as i64) * (scale as i64)) >> 16) as i32 / 2;
    if rescale_half {
        (((base as i64) * 0x8000) >> 16) as i32
    } else {
        base
    }
}

/// Per-car-type parameter region stride (`0xcd0`; matches the G02 drivetrain
/// param region and the damage shed-level array).
pub const CAR_TYPE_PARAM_STRIDE: usize = 0xcd0;

/// Per-car-type damage-region array stride (`0x334`; the per-region damage,
/// shed-flag, and max-torque arrays).
pub const DAMAGE_REGION_STRIDE: usize = 0x334;

/// Base index of a car type's parameter region (`car_type * 0xcd0`).
pub fn car_type_param_base(car_type: usize) -> usize {
    car_type * CAR_TYPE_PARAM_STRIDE
}

/// Base index of a car type's damage-region array (`car_type * 0x334`).
pub fn damage_region_base(car_type: usize) -> usize {
    car_type * DAMAGE_REGION_STRIDE
}

/// Select the nearest not-yet-shed panel to an impact point, reproducing the
/// panel-shed selection in `FUN_0044a770` (the closest region whose shed flag is
/// clear). Returns the panel index, or `None` when every panel is already shed.
/// The shed-level progression itself uses [`crate::collision::next_damage_level`]
/// against the recovered `0x3333`/`0x6666` thresholds.
pub fn nearest_unshed_panel(impact: Vec3, panels: &[(Vec3, bool)]) -> Option<usize> {
    panels
        .iter()
        .enumerate()
        .filter(|(_, (_, shed))| !shed)
        .min_by_key(|(_, (position, _))| impact.sub(*position).length().raw())
        .map(|(index, _)| index)
}

/// Number of resource pointers released by the car-data destructor.
pub const CAR_DATA_RESOURCE_SLOTS: usize = 16;

/// Release non-null car-data resources in ascending slot order and clear every
/// released slot (`0x481960`). Returns the number of releases, allowing a host
/// to verify the same ownership events without native pointers.
pub fn release_car_data_slots<T>(slots: &mut [Option<T>]) -> usize {
    let mut released = 0;
    for slot in slots {
        if slot.take().is_some() {
            released += 1;
        }
    }
    released
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_point::Fixed16;

    #[test]
    fn model_index_is_the_low_nibble() {
        assert_eq!(car_model_index(0x00), 0);
        assert_eq!(car_model_index(0x0d), 13);
        // High bits (other settings) are ignored.
        assert_eq!(car_model_index(0xf3), 3);
        assert!(is_valid_car_model(0));
        assert!(is_valid_car_model(13));
        assert!(!is_valid_car_model(14));
        assert!(!is_valid_car_model(15));
    }

    #[test]
    fn player_and_state_offsets_stride() {
        assert_eq!(player_settings_offset(0), 0);
        assert_eq!(player_settings_offset(1), 0x9c);
        assert_eq!(car_state_offset(1), 0x768);
    }

    #[test]
    fn body_and_reflection_paths() {
        // The car code is caller-supplied (not vendored); use a placeholder.
        assert_eq!(
            car_body_path("XYZ", false, false),
            r"OBJECTS\CARS\XYZDAY.C3D"
        );
        assert_eq!(
            car_body_path("XYZ", true, false),
            r"OBJECTS\CARS\XYZNIGHT.C3D"
        );
        // Low detail folds the day/night distinction into the suffix.
        assert_eq!(
            car_body_path("XYZ", false, true),
            r"OBJECTS\CARS\XYZdaymed.C3D"
        );
        assert_eq!(car_body_path("XYZ", true, true), r"OBJECTS\CARS\XYZmed.C3D");
        assert_eq!(
            car_reflection_path("XYZ", false),
            r"OBJECTS\CARS\XYZDAYref.c3d"
        );
        assert_eq!(car_reflection_map_path(), r"OBJECTS\CARS\ref_map.bmp");
    }

    #[test]
    fn in_race_body_branches_and_reflection_keep_their_observed_loader_policies() {
        use crate::c3d_mesh::node_mesh_vertex_type;
        use crate::c3d_render::VERTEXTYPE_LVERTEX;

        let keyed = in_race_car_load_policy(InRaceBodyLoadBranch::KeyedMode5, false);
        assert_eq!(keyed.body_mode, IN_RACE_KEYED_LOAD_MODE);
        assert_eq!(keyed.reflection_mode, IN_RACE_KEYED_LOAD_MODE);
        assert_eq!(keyed.caller_option, 1);
        assert_eq!(keyed.load_flags, IN_RACE_DAY_LOAD_FLAGS);
        assert_eq!(keyed.body_source_color_key, Some(0));
        assert_eq!(keyed.reflection_source_color_key, Some(0));
        assert!(keyed.body_sets_actor_alpha_ff);
        assert_eq!(node_mesh_vertex_type(keyed.load_flags), VERTEXTYPE_LVERTEX);

        let unkeyed = in_race_car_load_policy(InRaceBodyLoadBranch::UnkeyedMode6, true);
        assert_eq!(unkeyed.body_mode, IN_RACE_UNKEYED_LOAD_MODE);
        assert_eq!(unkeyed.reflection_mode, IN_RACE_KEYED_LOAD_MODE);
        assert_eq!(unkeyed.load_flags, IN_RACE_NIGHT_LOAD_FLAGS);
        assert_eq!(unkeyed.body_source_color_key, None);
        assert_eq!(unkeyed.reflection_source_color_key, Some(0));
        assert!(!unkeyed.body_sets_actor_alpha_ff);
        assert_eq!(
            node_mesh_vertex_type(unkeyed.load_flags),
            VERTEXTYPE_LVERTEX
        );
    }

    #[test]
    fn in_race_body_branch_follows_the_recovered_runtime_predicate() {
        let branch = |global_d3ca88_nonzero,
                      query_485bb0_nonzero,
                      query_401060_nonzero,
                      global_514478_nonzero| {
            in_race_body_load_branch(InRaceBodyLoadContext {
                global_d3ca88_nonzero,
                query_485bb0_nonzero,
                query_401060_nonzero,
                global_514478_nonzero,
            })
        };

        assert_eq!(
            branch(false, true, false, false),
            InRaceBodyLoadBranch::KeyedMode5
        );
        assert_eq!(
            branch(false, false, true, false),
            InRaceBodyLoadBranch::KeyedMode5
        );
        assert_eq!(
            branch(false, false, false, true),
            InRaceBodyLoadBranch::KeyedMode5
        );
        assert_eq!(
            branch(false, false, false, false),
            InRaceBodyLoadBranch::UnkeyedMode6
        );
        assert_eq!(
            branch(true, true, true, true),
            InRaceBodyLoadBranch::UnkeyedMode6
        );
    }

    fn sample_car() -> CarSceneNode {
        // root(0) -> [ body(10) -> [ wheel1(1), wheel2(2) ], axle(5) -> [ wheel3(3),
        //   wheel4(4), size(6) ] ]
        CarSceneNode::new(
            0,
            vec![
                CarSceneNode::new(10, vec![CarSceneNode::leaf(1), CarSceneNode::leaf(2)]),
                CarSceneNode::new(
                    5,
                    vec![
                        CarSceneNode::leaf(3),
                        CarSceneNode::leaf(4),
                        CarSceneNode::leaf(WHEEL_RADIUS_NODE_ID),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn node_search_is_preorder_and_masked() {
        let car = sample_car();
        assert_eq!(find_node_by_id(&car, 0).unwrap().id, 0);
        assert_eq!(find_node_by_id(&car, 3).unwrap().id, 3);
        assert_eq!(find_node_by_id(&car, WHEEL_RADIUS_NODE_ID).unwrap().id, 6);
        assert!(find_node_by_id(&car, 99).is_none());
        // High bits above the 12-bit id are masked off before comparison.
        assert_eq!(find_node_by_id(&car, 0x1000 | 2).unwrap().id, 2);
    }

    #[test]
    fn wheels_resolve_to_the_four_ids() {
        let car = sample_car();
        let wheels = find_wheels(&car);
        assert_eq!(wheels[0].unwrap().id, 1);
        assert_eq!(wheels[1].unwrap().id, 2);
        assert_eq!(wheels[2].unwrap().id, 3);
        assert_eq!(wheels[3].unwrap().id, 4);
        // A car missing a wheel node reports None for it.
        let stripped = CarSceneNode::new(0, vec![CarSceneNode::leaf(1)]);
        let w = find_wheels(&stripped);
        assert!(w[0].is_some());
        assert!(w[1].is_none());
    }

    #[test]
    fn wheel_radius_is_q16_half() {
        // size = 2.0, scale = 1.0 -> 2.0, halved -> 1.0 (0x10000).
        let size = Fixed16::from_int(2).raw();
        let scale = Fixed16::ONE.raw();
        assert_eq!(wheel_radius(size, scale, false), 0x1_0000);
        // With the extra 0x8000 (0.5) rescale -> 0.5 (0x8000).
        assert_eq!(wheel_radius(size, scale, true), 0x8000);
    }

    #[test]
    fn nearest_unshed_panel_skips_shed_and_picks_closest() {
        let fx = |n: i32| Fixed16::from_int(n);
        let impact = Vec3::new(fx(0), fx(0), fx(0));
        let panels = vec![
            (Vec3::new(fx(10), fx(0), fx(0)), false), // distance 10
            (Vec3::new(fx(3), fx(0), fx(0)), false),  // distance 3 (closest unshed)
            (Vec3::new(fx(1), fx(0), fx(0)), true),   // distance 1 but already shed
        ];
        assert_eq!(nearest_unshed_panel(impact, &panels), Some(1));
        // All shed -> None.
        let all_shed: Vec<_> = panels.iter().map(|(p, _)| (*p, true)).collect();
        assert_eq!(nearest_unshed_panel(impact, &all_shed), None);
        // Empty -> None.
        assert_eq!(nearest_unshed_panel(impact, &[]), None);
    }

    #[test]
    fn car_data_release_clears_only_present_slots() {
        let mut slots = vec![Some(10), None, Some(30), None];
        assert_eq!(release_car_data_slots(&mut slots), 2);
        assert_eq!(slots, vec![None, None, None, None]);
        assert_eq!(release_car_data_slots(&mut slots), 0);
    }

    #[test]
    fn damage_region_addressing_strides() {
        assert_eq!(car_type_param_base(0), 0);
        assert_eq!(car_type_param_base(1), 0xcd0);
        assert_eq!(damage_region_base(1), 0x334);
        assert_eq!(damage_region_base(2), 0x668);
    }
}
