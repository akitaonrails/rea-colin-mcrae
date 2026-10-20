//! Pure rendering contract for the recovered root-menu visuals.
//!
//! Commands describe ordering and recovered geometry only. Asset lookup and
//! pixel drawing remain backend responsibilities. Root entry text indices are
//! intentionally absent: static analysis showed that this screen uses
//! bitmap/C3D visuals, not the generic entry-label renderer.

use std::fmt;

use crate::{
    display::DisplayExtent,
    font_assets::FontStyle,
    frontend::{FrontendPhase, ROOT_FOOTER_TEXT_INDEX, RootSelection},
    localization::Language,
    root_menu_bitmap_assets::RootMenuBitmapSlot,
    root_menu_bitmap_raster::UploadedRootMenuBitmapPool,
    root_menu_c3d_assets::RootMenuC3dSlot,
};

pub const ROOT_ENTRY_COUNT: usize = 7;
pub const CENTER_VISUAL_COUNT: usize = 6;
pub const ROOT_FOOTER_POSITION: Point = Point::new(320, 470);
pub const ROOT_FOOTER_FLAGS: u8 = 0x22;
pub const CHROME_ANIMATION_SOURCE: Rect = Rect::new(0, 0, 400, 400);
pub const CHROME_ANIMATION_DESTINATION: Point = Point::new(128, 40);
pub const CENTER_ICON_OFFSET: Point = Point::new(-88, -116);
pub const ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16: i32 = 0x8000;
pub const ROOT_INITIAL_VERTICAL_TRANSFORM_Q16: i32 = 0;
pub const C3D_ANGLE_DELTA: u16 = 8;
pub const ROOT_FOOTER_FONT_STYLE: FontStyle = FontStyle::GreenInfo;
pub const ROOT_DORMANT_ENTRY_FONT_STYLE: FontStyle = FontStyle::Options;
pub const ROOT_DORMANT_TITLE_FONT_STYLE: FontStyle = FontStyle::RedDynamic;

pub const CENTER_VISUAL_BY_SELECTION: [usize; ROOT_ENTRY_COUNT] = [0, 0, 1, 2, 3, 4, 5];
pub const ANIMATION_BITMAP_SLOTS: [RootMenuBitmapSlot; ROOT_ENTRY_COUNT] = [
    RootMenuBitmapSlot::Animation0,
    RootMenuBitmapSlot::Animation1,
    RootMenuBitmapSlot::Animation2,
    RootMenuBitmapSlot::Animation3,
    RootMenuBitmapSlot::Animation4,
    RootMenuBitmapSlot::Animation5,
    RootMenuBitmapSlot::Animation6,
];
pub const CENTER_BITMAP_SLOTS: [RootMenuBitmapSlot; CENTER_VISUAL_COUNT] = [
    RootMenuBitmapSlot::ChampionshipGlobe,
    RootMenuBitmapSlot::TimeTrialGlobe,
    RootMenuBitmapSlot::Cone,
    RootMenuBitmapSlot::Spanner,
    RootMenuBitmapSlot::Disk,
    RootMenuBitmapSlot::Quit,
];

pub const CHROME_SPECS: [ChromeSpec; ROOT_ENTRY_COUNT] = [
    ChromeSpec::new(Rect::new(0, 0, 256, 45), Point::new(178, 394), 0x0000),
    ChromeSpec::new(Rect::new(0, 46, 256, 31), Point::new(221, 410), 0x293e),
    ChromeSpec::new(Rect::new(0, 78, 256, 35), Point::new(208, 400), 0x493e),
    ChromeSpec::new(Rect::new(0, 114, 256, 42), Point::new(188, 395), 0x7333),
    ChromeSpec::new(Rect::new(0, 157, 256, 32), Point::new(210, 405), 0x9ddd),
    ChromeSpec::new(Rect::new(0, 190, 256, 28), Point::new(226, 409), 0xbd27),
    ChromeSpec::new(Rect::new(0, 219, 256, 37), Point::new(233, 406), 0xdb05),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    #[must_use]
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    #[must_use]
    const fn offset(self, x: i32, y: i32) -> Self {
        Self::new(self.x + x, self.y + y)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChromeSpec {
    pub source: Rect,
    pub destination: Point,
    pub gate_q16: i32,
}

impl ChromeSpec {
    const fn new(source: Rect, destination: Point, gate_q16: i32) -> Self {
        Self {
            source,
            destination,
            gate_q16,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootVisualAssets {
    /// Decoded source dimensions used by the recovered full-rectangle guard.
    pub backdrop_extent: DisplayExtent,
    pub shadow_loaded: bool,
    pub animations_loaded: [bool; ROOT_ENTRY_COUNT],
    pub selected_chrome_loaded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootVisualAssetError {
    MissingRequiredBitmap(RootMenuBitmapSlot),
    BackdropExtentOutOfRange { width: u32, height: u32 },
}

impl fmt::Display for RootVisualAssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredBitmap(slot) => {
                write!(
                    formatter,
                    "required root visual bitmap {slot:?} is not loaded"
                )
            }
            Self::BackdropExtentOutOfRange { width, height } => write!(
                formatter,
                "root backdrop extent {width}x{height} is outside the positive u16 range"
            ),
        }
    }
}

impl std::error::Error for RootVisualAssetError {}

impl RootVisualAssets {
    pub fn from_bitmap_pool(
        pool: &UploadedRootMenuBitmapPool,
    ) -> Result<Self, RootVisualAssetError> {
        for slot in [
            RootMenuBitmapSlot::Backdrop,
            RootMenuBitmapSlot::Selection,
            RootMenuBitmapSlot::ChampionshipGlobe,
            RootMenuBitmapSlot::TimeTrialGlobe,
            RootMenuBitmapSlot::Cone,
            RootMenuBitmapSlot::Spanner,
            RootMenuBitmapSlot::Disk,
            RootMenuBitmapSlot::Quit,
        ] {
            if !pool.is_loaded(slot) {
                return Err(RootVisualAssetError::MissingRequiredBitmap(slot));
            }
        }
        let (width, height) = pool
            .extent(RootMenuBitmapSlot::Backdrop)
            .expect("required backdrop presence was checked");
        let backdrop_extent = DisplayExtent {
            width: u16::try_from(width)
                .ok()
                .filter(|value| *value > 0)
                .ok_or(RootVisualAssetError::BackdropExtentOutOfRange { width, height })?,
            height: u16::try_from(height)
                .ok()
                .filter(|value| *value > 0)
                .ok_or(RootVisualAssetError::BackdropExtentOutOfRange { width, height })?,
        };
        Ok(Self {
            backdrop_extent,
            shadow_loaded: pool.is_loaded(RootMenuBitmapSlot::MainShadow),
            animations_loaded: ANIMATION_BITMAP_SLOTS.map(|slot| pool.is_loaded(slot)),
            selected_chrome_loaded: pool.is_loaded(RootMenuBitmapSlot::MainSelected),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootRenderInput {
    pub phase: FrontendPhase,
    pub selection: RootSelection,
    pub animation_position_q16: i32,
    pub language: Language,
    pub viewport_extent: DisplayExtent,
    /// Neutral names preserve the two recovered Q16 fields without assigning
    /// unproven camera/scroll semantics.
    pub horizontal_transform_q16: i32,
    pub vertical_transform_q16: i32,
    /// Recovered entry-callback gate used during the enter transition.
    pub entry_visuals_suppressed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderCommand {
    Backdrop {
        slot: RootMenuBitmapSlot,
        source: Rect,
        destination: Point,
    },
    ChromeShadow {
        slot: RootMenuBitmapSlot,
        entry: u8,
        source: Rect,
        destination: Point,
    },
    ChromeAnimation {
        slot: RootMenuBitmapSlot,
        entry: u8,
        source: Rect,
        destination: Point,
    },
    SelectedChrome {
        slot: RootMenuBitmapSlot,
        entry: u8,
        source: Rect,
        destination: Point,
    },
    FooterText {
        text_index: usize,
        position: Point,
        flags: u8,
    },
    CenterIcon {
        slot: RootMenuBitmapSlot,
        visual_index: u8,
        destination: Point,
    },
    C3dObject {
        slot: RootMenuC3dSlot,
        angle_delta: u16,
    },
    ComposeAndPresent,
}

#[must_use]
const fn chrome_source_rect(language: Language, index: usize) -> Rect {
    match (language, index) {
        (Language::Spanish | Language::Italian, 1) => Rect::new(0, 46, 256, 26),
        (Language::Spanish | Language::Italian, 2) => Rect::new(0, 72, 256, 41),
        _ => CHROME_SPECS[index].source,
    }
}

/// Produces the recovered root-menu draw order without performing any I/O.
///
/// Missing optional chrome surfaces omit only their corresponding commands.
/// C3D commands retain stable slot identity; parsed-object availability is
/// resolved by the frame executor. Reconstruction requires backdrop and center
/// surfaces, although one dynamically constructed center path was unchecked by
/// the original loader.
/// Chrome and footer drawing is restricted to numeric frontend phase 1; the
/// selected center visual remains part of entry dispatch in other phases unless
/// the recovered transition-suppression gate is set.
#[must_use]
pub fn root_render_transcript(
    input: RootRenderInput,
    assets: &RootVisualAssets,
) -> Vec<RenderCommand> {
    let mut commands = Vec::with_capacity(9);

    let backdrop_source = backdrop_source(input);
    if source_fits(backdrop_source, assets.backdrop_extent) {
        commands.push(RenderCommand::Backdrop {
            slot: RootMenuBitmapSlot::Backdrop,
            source: backdrop_source,
            destination: Point::new(0, 0),
        });
    }

    if input.phase == FrontendPhase::Interactive {
        let position_bucket = animation_bucket(input.animation_position_q16);
        for (index, spec) in CHROME_SPECS.iter().enumerate() {
            if animation_bucket(spec.gate_q16) != position_bucket {
                continue;
            }
            let source = chrome_source_rect(input.language, index);
            if assets.shadow_loaded {
                commands.push(RenderCommand::ChromeShadow {
                    slot: RootMenuBitmapSlot::MainShadow,
                    entry: index as u8,
                    source,
                    destination: spec.destination.offset(2, 2),
                });
            }
            if assets.selected_chrome_loaded {
                if assets.animations_loaded[index] {
                    commands.push(RenderCommand::ChromeAnimation {
                        slot: ANIMATION_BITMAP_SLOTS[index],
                        entry: index as u8,
                        source: CHROME_ANIMATION_SOURCE,
                        destination: CHROME_ANIMATION_DESTINATION,
                    });
                }
                commands.push(RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: index as u8,
                    source,
                    destination: spec.destination,
                });
            }
        }
        commands.push(RenderCommand::FooterText {
            text_index: ROOT_FOOTER_TEXT_INDEX,
            position: ROOT_FOOTER_POSITION,
            flags: ROOT_FOOTER_FLAGS,
        });
    }

    if !input.entry_visuals_suppressed {
        let selection = usize::from(input.selection.index());
        let visual = CENTER_VISUAL_BY_SELECTION[selection];
        commands.push(RenderCommand::CenterIcon {
            slot: CENTER_BITMAP_SLOTS[visual],
            visual_index: visual as u8,
            destination: center_icon_destination(input),
        });
        if let Some(slot) = RootMenuC3dSlot::for_selection(input.selection) {
            commands.push(RenderCommand::C3dObject {
                slot,
                angle_delta: C3D_ANGLE_DELTA,
            });
        }
    }

    commands.push(RenderCommand::ComposeAndPresent);
    commands
}

#[must_use]
fn backdrop_source(input: RootRenderInput) -> Rect {
    Rect::new(
        q16_scale(input.viewport_extent.width, input.horizontal_transform_q16).wrapping_add(320),
        q16_scale(input.viewport_extent.height, input.vertical_transform_q16).wrapping_add(240),
        input.viewport_extent.width,
        input.viewport_extent.height,
    )
}

#[must_use]
fn center_icon_destination(input: RootRenderInput) -> Point {
    Point::new(
        q16_scale(
            input.viewport_extent.width,
            0x10000i32.wrapping_sub(input.horizontal_transform_q16),
        )
        .wrapping_add(CENTER_ICON_OFFSET.x),
        q16_scale(
            input.viewport_extent.height,
            0x8000i32.wrapping_sub(input.vertical_transform_q16),
        )
        .wrapping_add(CENTER_ICON_OFFSET.y),
    )
}

#[must_use]
fn source_fits(source: Rect, extent: DisplayExtent) -> bool {
    source.x >= 0
        && source.y >= 0
        && source
            .x
            .checked_add(i32::from(source.width))
            .is_some_and(|right| right <= i32::from(extent.width))
        && source
            .y
            .checked_add(i32::from(source.height))
            .is_some_and(|bottom| bottom <= i32::from(extent.height))
}

fn q16_scale(value: u16, factor_q16: i32) -> i32 {
    i32::from(value).wrapping_mul(factor_q16) >> 16
}

const fn animation_bucket(position_q16: i32) -> i32 {
    position_q16.wrapping_mul(360) >> 16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{frontend::RootSelection, localization::Language};

    const EXTENT: DisplayExtent = DisplayExtent {
        width: 640,
        height: 480,
    };

    fn complete_assets() -> RootVisualAssets {
        RootVisualAssets {
            backdrop_extent: DisplayExtent {
                width: 1280,
                height: 720,
            },
            shadow_loaded: true,
            animations_loaded: [true; ROOT_ENTRY_COUNT],
            selected_chrome_loaded: true,
        }
    }

    fn input(selection: u8) -> RootRenderInput {
        RootRenderInput {
            phase: FrontendPhase::Interactive,
            selection: RootSelection::new(selection).unwrap(),
            animation_position_q16: CHROME_SPECS[usize::from(selection)].gate_q16,
            language: Language::English,
            viewport_extent: EXTENT,
            horizontal_transform_q16: ROOT_INITIAL_HORIZONTAL_TRANSFORM_Q16,
            vertical_transform_q16: ROOT_INITIAL_VERTICAL_TRANSFORM_Q16,
            entry_visuals_suppressed: false,
        }
    }

    fn backdrop() -> RenderCommand {
        RenderCommand::Backdrop {
            slot: RootMenuBitmapSlot::Backdrop,
            source: Rect::new(640, 240, 640, 480),
            destination: Point::new(0, 0),
        }
    }

    fn center_icon(visual_index: u8) -> RenderCommand {
        RenderCommand::CenterIcon {
            slot: CENTER_BITMAP_SLOTS[usize::from(visual_index)],
            visual_index,
            destination: Point::new(232, 124),
        }
    }

    #[test]
    fn recovered_chrome_tables_are_exact() {
        let expected = [
            (Rect::new(0, 0, 256, 45), Point::new(178, 394), 0x0000),
            (Rect::new(0, 46, 256, 31), Point::new(221, 410), 0x293e),
            (Rect::new(0, 78, 256, 35), Point::new(208, 400), 0x493e),
            (Rect::new(0, 114, 256, 42), Point::new(188, 395), 0x7333),
            (Rect::new(0, 157, 256, 32), Point::new(210, 405), 0x9ddd),
            (Rect::new(0, 190, 256, 28), Point::new(226, 409), 0xbd27),
            (Rect::new(0, 219, 256, 37), Point::new(233, 406), 0xdb05),
        ];

        for (spec, (source, destination, gate_q16)) in CHROME_SPECS.iter().zip(expected) {
            assert_eq!(spec.source, source);
            assert_eq!(spec.destination, destination);
            assert_eq!(spec.gate_q16, gate_q16);
        }
        assert_eq!(CHROME_ANIMATION_SOURCE, Rect::new(0, 0, 400, 400));
        assert_eq!(CHROME_ANIMATION_DESTINATION, Point::new(128, 40));
        assert_eq!(
            ANIMATION_BITMAP_SLOTS,
            [
                RootMenuBitmapSlot::Animation0,
                RootMenuBitmapSlot::Animation1,
                RootMenuBitmapSlot::Animation2,
                RootMenuBitmapSlot::Animation3,
                RootMenuBitmapSlot::Animation4,
                RootMenuBitmapSlot::Animation5,
                RootMenuBitmapSlot::Animation6,
            ]
        );
        assert_eq!(
            CENTER_BITMAP_SLOTS,
            [
                RootMenuBitmapSlot::ChampionshipGlobe,
                RootMenuBitmapSlot::TimeTrialGlobe,
                RootMenuBitmapSlot::Cone,
                RootMenuBitmapSlot::Spanner,
                RootMenuBitmapSlot::Disk,
                RootMenuBitmapSlot::Quit,
            ]
        );
    }

    #[test]
    fn root_font_globals_map_to_exact_loaded_styles() {
        assert_eq!(ROOT_FOOTER_FONT_STYLE, FontStyle::GreenInfo);
        assert_eq!(ROOT_DORMANT_ENTRY_FONT_STYLE, FontStyle::Options);
        assert_eq!(ROOT_DORMANT_TITLE_FONT_STYLE, FontStyle::RedDynamic);
    }

    #[test]
    fn spanish_and_italian_apply_only_the_two_recovered_overrides() {
        for language in [Language::Spanish, Language::Italian] {
            assert_eq!(chrome_source_rect(language, 1), Rect::new(0, 46, 256, 26));
            assert_eq!(chrome_source_rect(language, 2), Rect::new(0, 72, 256, 41));
            for index in [0, 3, 4, 5, 6] {
                assert_eq!(
                    chrome_source_rect(language, index),
                    CHROME_SPECS[index].source
                );
            }
        }

        for language in [Language::English, Language::French, Language::German] {
            for (index, spec) in CHROME_SPECS.iter().enumerate() {
                assert_eq!(chrome_source_rect(language, index), spec.source);
            }
        }
    }

    #[test]
    fn every_steady_selection_has_the_exact_ordered_transcript() {
        let assets = complete_assets();
        for selection in 0..ROOT_ENTRY_COUNT as u8 {
            let commands = root_render_transcript(input(selection), &assets);
            let visual = CENTER_VISUAL_BY_SELECTION[usize::from(selection)];
            let expected_object =
                RootMenuC3dSlot::for_selection(input(selection).selection).map(|slot| {
                    RenderCommand::C3dObject {
                        slot,
                        angle_delta: C3D_ANGLE_DELTA,
                    }
                });
            let mut expected = vec![
                backdrop(),
                RenderCommand::ChromeShadow {
                    slot: RootMenuBitmapSlot::MainShadow,
                    entry: selection,
                    source: CHROME_SPECS[usize::from(selection)].source,
                    destination: CHROME_SPECS[usize::from(selection)]
                        .destination
                        .offset(2, 2),
                },
                RenderCommand::ChromeAnimation {
                    slot: ANIMATION_BITMAP_SLOTS[usize::from(selection)],
                    entry: selection,
                    source: CHROME_ANIMATION_SOURCE,
                    destination: CHROME_ANIMATION_DESTINATION,
                },
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: selection,
                    source: CHROME_SPECS[usize::from(selection)].source,
                    destination: CHROME_SPECS[usize::from(selection)].destination,
                },
                RenderCommand::FooterText {
                    text_index: ROOT_FOOTER_TEXT_INDEX,
                    position: ROOT_FOOTER_POSITION,
                    flags: ROOT_FOOTER_FLAGS,
                },
                center_icon(visual as u8),
            ];
            expected.extend(expected_object);
            expected.push(RenderCommand::ComposeAndPresent);
            assert_eq!(commands, expected, "selection {selection}");
            assert!(!commands.iter().any(|command| matches!(
                command,
                RenderCommand::Backdrop {
                    slot: RootMenuBitmapSlot::Selection,
                    ..
                } | RenderCommand::ChromeShadow {
                    slot: RootMenuBitmapSlot::Selection,
                    ..
                } | RenderCommand::ChromeAnimation {
                    slot: RootMenuBitmapSlot::Selection,
                    ..
                } | RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::Selection,
                    ..
                } | RenderCommand::CenterIcon {
                    slot: RootMenuBitmapSlot::Selection,
                    ..
                }
            )));
        }
    }

    #[test]
    fn chrome_and_footer_are_phase_gated_but_selected_visual_is_not() {
        let assets = complete_assets();
        let mut render_input = input(3);
        render_input.phase = FrontendPhase::AdvanceToPending;
        assert_eq!(
            root_render_transcript(render_input, &assets),
            vec![
                backdrop(),
                center_icon(2),
                RenderCommand::C3dObject {
                    slot: RootMenuC3dSlot::RallySchool,
                    angle_delta: C3D_ANGLE_DELTA,
                },
                RenderCommand::ComposeAndPresent,
            ]
        );
    }

    #[test]
    fn quantized_gate_can_match_or_omit_chrome_without_affecting_footer() {
        let assets = complete_assets();
        let mut render_input = input(0);
        render_input.animation_position_q16 = CHROME_SPECS[1].gate_q16;
        let commands = root_render_transcript(render_input, &assets);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::SelectedChrome { entry: 1, .. }))
        );
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, RenderCommand::SelectedChrome { entry: 0, .. }))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::FooterText { .. }))
        );
    }

    #[test]
    fn optional_bitmaps_skip_only_their_layers_and_preserve_order() {
        let mut assets = complete_assets();
        assets.shadow_loaded = false;
        assets.animations_loaded[2] = false;
        assert_eq!(
            root_render_transcript(input(2), &assets),
            vec![
                backdrop(),
                RenderCommand::SelectedChrome {
                    slot: RootMenuBitmapSlot::MainSelected,
                    entry: 2,
                    source: Rect::new(0, 78, 256, 35),
                    destination: Point::new(208, 400),
                },
                RenderCommand::FooterText {
                    text_index: ROOT_FOOTER_TEXT_INDEX,
                    position: ROOT_FOOTER_POSITION,
                    flags: ROOT_FOOTER_FLAGS,
                },
                center_icon(1),
                RenderCommand::C3dObject {
                    slot: RootMenuC3dSlot::TimeTrial,
                    angle_delta: C3D_ANGLE_DELTA,
                },
                RenderCommand::ComposeAndPresent,
            ]
        );
    }

    #[test]
    fn selected_chrome_gate_owns_animation_but_not_shadow() {
        let mut assets = complete_assets();
        assets.selected_chrome_loaded = false;
        let commands = root_render_transcript(input(4), &assets);
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::ChromeShadow { entry: 4, .. }))
        );
        assert!(!commands.iter().any(|command| matches!(
            command,
            RenderCommand::ChromeAnimation { entry: 4, .. }
                | RenderCommand::SelectedChrome { entry: 4, .. }
        )));
    }

    #[test]
    fn q16_fields_drive_exact_backdrop_crop_and_center_destination() {
        let mut assets = complete_assets();
        assets.backdrop_extent = DisplayExtent {
            width: 1320,
            height: 915,
        };
        let mut render_input = input(0);
        render_input.viewport_extent = DisplayExtent {
            width: 800,
            height: 600,
        };
        render_input.horizontal_transform_q16 = 0x4000;
        render_input.vertical_transform_q16 = 0x2000;
        let commands = root_render_transcript(render_input, &assets);
        assert_eq!(
            commands[0],
            RenderCommand::Backdrop {
                slot: RootMenuBitmapSlot::Backdrop,
                source: Rect::new(520, 315, 800, 600),
                destination: Point::new(0, 0),
            }
        );
        assert!(commands.iter().any(|command| {
            *command
                == RenderCommand::CenterIcon {
                    slot: RootMenuBitmapSlot::ChampionshipGlobe,
                    visual_index: 0,
                    destination: Point::new(512, 109),
                }
        }));
    }

    #[test]
    fn backdrop_draw_requires_the_complete_nonnegative_source_rectangle() {
        let render_input = input(0);
        let mut assets = complete_assets();
        assert!(matches!(
            root_render_transcript(render_input, &assets).first(),
            Some(RenderCommand::Backdrop { .. })
        ));

        assets.backdrop_extent.width -= 1;
        assert!(
            !root_render_transcript(render_input, &assets)
                .iter()
                .any(|command| matches!(command, RenderCommand::Backdrop { .. }))
        );

        assets.backdrop_extent.width += 1;
        assets.backdrop_extent.height -= 1;
        assert!(
            !root_render_transcript(render_input, &assets)
                .iter()
                .any(|command| matches!(command, RenderCommand::Backdrop { .. }))
        );

        assets.backdrop_extent.height += 1;
        let mut negative = render_input;
        negative.horizontal_transform_q16 = -0x10000;
        assert!(
            !root_render_transcript(negative, &assets)
                .iter()
                .any(|command| matches!(command, RenderCommand::Backdrop { .. }))
        );
    }

    #[test]
    fn q16_scaling_retains_signed_wrapping_arithmetic() {
        assert_eq!(q16_scale(u16::MAX, i32::MAX), 32_767);
        assert_eq!(q16_scale(u16::MAX, i32::MIN), -32_768);
    }

    #[test]
    fn exit_uses_center_visual_five_without_a_c3d_object() {
        let commands = root_render_transcript(input(6), &complete_assets());
        assert!(commands.iter().any(|command| matches!(
            command,
            RenderCommand::CenterIcon {
                visual_index: 5,
                ..
            }
        )));
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, RenderCommand::C3dObject { .. }))
        );
    }

    #[test]
    fn transition_suppression_omits_center_icon_and_c3d_only() {
        let mut render_input = input(2);
        render_input.entry_visuals_suppressed = true;
        let commands = root_render_transcript(render_input, &complete_assets());
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::SelectedChrome { entry: 2, .. }))
        );
        assert!(
            commands
                .iter()
                .any(|command| matches!(command, RenderCommand::FooterText { .. }))
        );
        assert!(!commands.iter().any(|command| matches!(
            command,
            RenderCommand::CenterIcon { .. } | RenderCommand::C3dObject { .. }
        )));
        assert_eq!(commands.last(), Some(&RenderCommand::ComposeAndPresent));
    }

    #[test]
    fn transcript_has_no_generic_root_label_command() {
        // The command vocabulary deliberately has no entry-label variant. The only
        // text operation recovered on this screen is the footer.
        let commands = root_render_transcript(input(4), &complete_assets());
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(command, RenderCommand::FooterText { .. }))
                .count(),
            1
        );
    }
}
