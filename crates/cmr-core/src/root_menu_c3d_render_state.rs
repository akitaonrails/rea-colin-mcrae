//! Recovered fixed-function render-state initialization used by root C3D.
//!
//! The original frontend reapplies this state program after its Direct3D
//! device has been created. Two values depend on graphics-runtime gates, so
//! callers must select those inputs explicitly instead of inheriting host API
//! defaults. Numeric state/value pairs are retained as the compatibility ABI;
//! the names document their `IDirect3DDevice2` enum interpretation.

/// Graphics branch selected by the legacy renderer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dGraphicsBranch {
    /// Palettized/indexed resource branch (`renderer + 0x28 == 0`).
    Indexed,
    /// Alternate resource branch (`renderer + 0x28 != 0`).
    Alternate,
}

/// Bit sampled by the game from the legacy raster-capability word.
///
/// This name intentionally describes the recovered game gate, not an asserted
/// meaning for the underlying Direct3D capability bit.
pub const ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT: u32 = 0x0000_8000;

/// Blend-factor values submitted whenever the root C3D state is initialized.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum C3dBlendFactor {
    SourceAlpha = 5,
    InverseSourceAlpha = 6,
}

/// What this recovered gate establishes about alpha testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dAlphaTestKnowledge {
    /// The observed gate controls blending and depth-clear policy only; alpha
    /// testing has not been recovered from this path.
    NotRecoveredFromLegacyGate,
}

/// Coupled behavior selected by the game's legacy raster-capability gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dLegacyAlphaContract {
    pub alpha_blend_enabled: bool,
    pub clear_depth_before_scene: bool,
    pub source_blend: C3dBlendFactor,
    pub destination_blend: C3dBlendFactor,
    pub alpha_test: C3dAlphaTestKnowledge,
}

/// Decodes the observed bit test in the game's legacy raster-capability word.
///
/// No claim is made that bit `0x0000_8000` is itself an alpha capability. It
/// is only the bit the game uses to select these two coupled behaviors.
#[must_use]
pub const fn root_menu_c3d_legacy_alpha_contract(
    legacy_raster_capability_bits: u32,
) -> RootMenuC3dLegacyAlphaContract {
    let alpha_blend_enabled =
        legacy_raster_capability_bits & ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT != 0;
    RootMenuC3dLegacyAlphaContract {
        alpha_blend_enabled,
        clear_depth_before_scene: !alpha_blend_enabled,
        source_blend: C3dBlendFactor::SourceAlpha,
        destination_blend: C3dBlendFactor::InverseSourceAlpha,
        alpha_test: C3dAlphaTestKnowledge::NotRecoveredFromLegacyGate,
    }
}

/// Runtime inputs sampled by the recovered renderer initialization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dRenderStateInputs {
    pub graphics_branch: C3dGraphicsBranch,
    /// Value returned by the legacy runtime gate used for alpha blending.
    pub alpha_blend_enabled: bool,
    /// Whether the filter capability queried by the legacy helper is present.
    /// An unavailable capability substitutes value 1 on device submission.
    pub filter_capability_available: bool,
    /// Texture-handle value retained by the legacy submission cache.
    pub cached_texture_handle: u32,
    /// Requested filter value retained by the legacy submission cache.
    pub cached_filter_request: u32,
}

impl RootMenuC3dRenderStateInputs {
    /// Applies the recovered alpha gate while leaving unrelated inputs intact.
    #[must_use]
    pub const fn with_legacy_alpha_contract(
        mut self,
        contract: RootMenuC3dLegacyAlphaContract,
    ) -> Self {
        self.alpha_blend_enabled = contract.alpha_blend_enabled;
        self
    }
}

/// One ordered `SetRenderState(state, value)` submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct C3dRenderStateWrite {
    pub state: C3dRenderState,
    pub value: u32,
}

/// `IDirect3DDevice2` render-state numbers used by the recovered path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum C3dRenderState {
    TextureHandle = 1,
    Antialias = 2,
    TextureAddress = 3,
    TexturePerspective = 4,
    ZEnable = 7,
    FillMode = 8,
    ShadeMode = 9,
    ZWriteEnable = 14,
    TextureMag = 17,
    TextureMin = 18,
    SourceBlend = 19,
    DestinationBlend = 20,
    TextureMapBlend = 21,
    CullMode = 22,
    ZFunction = 23,
    DitherEnable = 26,
    AlphaBlendEnable = 27,
    FogEnable = 28,
    SpecularEnable = 29,
    StippledAlpha = 33,
    FogColor = 34,
    ColorKeyEnable = 41,
}

pub const ROOT_MENU_C3D_RENDER_STATE_BASE_WRITE_COUNT: usize = 20;
pub const ROOT_MENU_C3D_RENDER_STATE_MAX_WRITE_COUNT: usize = 23;
pub const ROOT_MENU_C3D_INITIAL_TEXTURE_HANDLE_CACHE: u32 = 0;
pub const ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE: u32 = 2;

/// Observable device action selected by the legacy filter-request cache.
///
/// The cache retains the requested value rather than the value actually
/// submitted after capability fallback. When the request already matches the
/// cache, this path performs no device write and therefore cannot establish
/// which filter value the device currently uses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum C3dFilterDeviceAction {
    /// Both texture magnification and minimization calls submit this value.
    /// The legacy path does not establish that the driver accepts either call.
    Submit { submitted_value: u32 },
    /// No device call occurs; the effective device value is inherited or
    /// otherwise unknown to this cache transition.
    PreserveInheritedOrUnknown,
}

impl C3dFilterDeviceAction {
    #[must_use]
    pub const fn device_write_occurs(self) -> bool {
        matches!(self, Self::Submit { .. })
    }

    #[must_use]
    pub const fn submitted_value(self) -> Option<u32> {
        match self {
            Self::Submit { submitted_value } => Some(submitted_value),
            Self::PreserveInheritedOrUnknown => None,
        }
    }
}

/// Result of applying one branch's filter request to the legacy request cache.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootMenuC3dFilterCacheOutcome {
    pub requested_filter: u32,
    pub device_action: C3dFilterDeviceAction,
    pub resulting_filter_request_cache: u32,
}

/// Models the recovered filter helper independently from the surrounding
/// render-state writes.
///
/// Capability fallback changes only the submitted value. The requested value
/// is always what the cache retains, including when request 2 falls back to
/// submitted value 1.
#[must_use]
pub const fn root_menu_c3d_filter_cache_outcome(
    graphics_branch: C3dGraphicsBranch,
    filter_capability_available: bool,
    cached_filter_request: u32,
) -> RootMenuC3dFilterCacheOutcome {
    let requested_filter = match graphics_branch {
        C3dGraphicsBranch::Indexed => 1,
        C3dGraphicsBranch::Alternate => 2,
    };
    let device_action = if cached_filter_request == requested_filter {
        C3dFilterDeviceAction::PreserveInheritedOrUnknown
    } else {
        C3dFilterDeviceAction::Submit {
            submitted_value: if filter_capability_available {
                requested_filter
            } else {
                1
            },
        }
    };
    RootMenuC3dFilterCacheOutcome {
        requested_filter,
        device_action,
        resulting_filter_request_cache: requested_filter,
    }
}

const fn write(state: C3dRenderState, value: u32) -> C3dRenderStateWrite {
    C3dRenderStateWrite { state, value }
}

/// Ordered device writes plus the two legacy cache values after initialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RootMenuC3dRenderStateProgram {
    pub writes: Vec<C3dRenderStateWrite>,
    pub resulting_texture_handle_cache: u32,
    pub resulting_filter_request_cache: u32,
}

/// Reconstructs the exact ordered state writes performed by the frontend
/// initialization used by root C3D.
#[must_use]
pub fn root_menu_c3d_render_state_program(
    inputs: RootMenuC3dRenderStateInputs,
) -> RootMenuC3dRenderStateProgram {
    let (shade_mode, texture_map_blend) = match inputs.graphics_branch {
        C3dGraphicsBranch::Indexed => (1, 7),
        C3dGraphicsBranch::Alternate => (2, 2),
    };
    let filter_outcome = root_menu_c3d_filter_cache_outcome(
        inputs.graphics_branch,
        inputs.filter_capability_available,
        inputs.cached_filter_request,
    );
    let alpha_blend_enabled = inputs.alpha_blend_enabled as u32;

    use C3dRenderState as State;
    let mut writes = Vec::with_capacity(ROOT_MENU_C3D_RENDER_STATE_MAX_WRITE_COUNT);
    writes.extend([
        write(State::ShadeMode, shade_mode),
        // The executable first enables blending, then immediately overwrites
        // it with the sampled runtime gate.
        write(State::AlphaBlendEnable, 1),
        write(State::AlphaBlendEnable, alpha_blend_enabled),
        write(State::ColorKeyEnable, 1),
        write(State::TextureAddress, 3),
        write(State::StippledAlpha, 0),
        write(State::SourceBlend, C3dBlendFactor::SourceAlpha as u32),
        write(
            State::DestinationBlend,
            C3dBlendFactor::InverseSourceAlpha as u32,
        ),
        write(State::TexturePerspective, 1),
        write(State::ZEnable, 1),
        write(State::ZWriteEnable, 1),
        write(State::ZFunction, 4),
        write(State::CullMode, 3),
        write(State::TextureMapBlend, texture_map_blend),
        write(State::FillMode, 3),
        write(State::DitherEnable, 1),
        write(State::SpecularEnable, 0),
        write(State::Antialias, 0),
        write(State::FogEnable, 0),
        write(State::FogColor, 0x00ff_ffff),
    ]);
    if inputs.cached_texture_handle != 0 {
        writes.push(write(State::TextureHandle, 0));
    }
    if let C3dFilterDeviceAction::Submit { submitted_value } = filter_outcome.device_action {
        writes.push(write(State::TextureMag, submitted_value));
        writes.push(write(State::TextureMin, submitted_value));
    }
    RootMenuC3dRenderStateProgram {
        writes,
        resulting_texture_handle_cache: 0,
        resulting_filter_request_cache: filter_outcome.resulting_filter_request_cache,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_filter_request_from_fresh_cache_submits_point_filtering() {
        let outcome = root_menu_c3d_filter_cache_outcome(
            C3dGraphicsBranch::Indexed,
            true,
            ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        );

        assert_eq!(outcome.requested_filter, 1);
        assert_eq!(
            outcome.device_action,
            C3dFilterDeviceAction::Submit { submitted_value: 1 }
        );
        assert!(outcome.device_action.device_write_occurs());
        assert_eq!(outcome.device_action.submitted_value(), Some(1));
        assert_eq!(outcome.resulting_filter_request_cache, 1);
    }

    #[test]
    fn alternate_filter_request_from_fresh_cache_preserves_unknown_device_state() {
        let outcome = root_menu_c3d_filter_cache_outcome(
            C3dGraphicsBranch::Alternate,
            true,
            ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        );

        assert_eq!(outcome.requested_filter, 2);
        assert_eq!(
            outcome.device_action,
            C3dFilterDeviceAction::PreserveInheritedOrUnknown
        );
        assert!(!outcome.device_action.device_write_occurs());
        assert_eq!(outcome.device_action.submitted_value(), None);
        assert_eq!(outcome.resulting_filter_request_cache, 2);
    }

    #[test]
    fn disturbed_alternate_filter_cache_submits_requested_value_when_supported() {
        let outcome = root_menu_c3d_filter_cache_outcome(C3dGraphicsBranch::Alternate, true, 1);

        assert_eq!(outcome.requested_filter, 2);
        assert_eq!(
            outcome.device_action,
            C3dFilterDeviceAction::Submit { submitted_value: 2 }
        );
        assert_eq!(outcome.device_action.submitted_value(), Some(2));
        assert_eq!(outcome.resulting_filter_request_cache, 2);
    }

    #[test]
    fn unsupported_alternate_filter_falls_back_but_caches_the_request() {
        let outcome = root_menu_c3d_filter_cache_outcome(C3dGraphicsBranch::Alternate, false, 1);

        assert_eq!(outcome.requested_filter, 2);
        assert_eq!(
            outcome.device_action,
            C3dFilterDeviceAction::Submit { submitted_value: 1 }
        );
        assert_eq!(outcome.device_action.submitted_value(), Some(1));
        assert_eq!(outcome.resulting_filter_request_cache, 2);
    }

    #[test]
    fn repeated_filter_request_suppresses_write_and_cannot_recover_effective_value() {
        let first = root_menu_c3d_filter_cache_outcome(C3dGraphicsBranch::Alternate, false, 1);
        assert_eq!(first.device_action.submitted_value(), Some(1));

        let repeated = root_menu_c3d_filter_cache_outcome(
            C3dGraphicsBranch::Alternate,
            true,
            first.resulting_filter_request_cache,
        );
        assert_eq!(repeated.requested_filter, 2);
        assert_eq!(
            repeated.device_action,
            C3dFilterDeviceAction::PreserveInheritedOrUnknown
        );
        assert_eq!(repeated.device_action.submitted_value(), None);
        assert_eq!(repeated.resulting_filter_request_cache, 2);
    }

    #[test]
    fn legacy_alpha_gate_couples_blending_and_pre_scene_depth_clear() {
        let disabled = root_menu_c3d_legacy_alpha_contract(0);
        assert!(!disabled.alpha_blend_enabled);
        assert!(disabled.clear_depth_before_scene);

        let enabled = root_menu_c3d_legacy_alpha_contract(ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT);
        assert!(enabled.alpha_blend_enabled);
        assert!(!enabled.clear_depth_before_scene);
    }

    #[test]
    fn legacy_alpha_gate_ignores_every_other_capability_bit() {
        let all_other_bits = !ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT;
        assert_eq!(
            root_menu_c3d_legacy_alpha_contract(all_other_bits),
            root_menu_c3d_legacy_alpha_contract(0)
        );
        assert_eq!(
            root_menu_c3d_legacy_alpha_contract(u32::MAX),
            root_menu_c3d_legacy_alpha_contract(ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT)
        );
    }

    #[test]
    fn legacy_alpha_contract_retains_fixed_blend_factors_and_unknown_test_boundary() {
        for bits in [0, ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT] {
            let contract = root_menu_c3d_legacy_alpha_contract(bits);
            assert_eq!(contract.source_blend, C3dBlendFactor::SourceAlpha);
            assert_eq!(contract.source_blend as u32, 5);
            assert_eq!(
                contract.destination_blend,
                C3dBlendFactor::InverseSourceAlpha
            );
            assert_eq!(contract.destination_blend as u32, 6);
            assert_eq!(
                contract.alpha_test,
                C3dAlphaTestKnowledge::NotRecoveredFromLegacyGate
            );
        }
    }

    #[test]
    fn render_state_inputs_can_consume_the_legacy_alpha_contract() {
        let contract = root_menu_c3d_legacy_alpha_contract(ROOT_MENU_C3D_LEGACY_ALPHA_GATE_BIT);
        let inputs = RootMenuC3dRenderStateInputs {
            graphics_branch: C3dGraphicsBranch::Indexed,
            alpha_blend_enabled: false,
            filter_capability_available: true,
            cached_texture_handle: 0,
            cached_filter_request: 1,
        }
        .with_legacy_alpha_contract(contract);
        let program = root_menu_c3d_render_state_program(inputs);

        assert_eq!(
            program.writes[2],
            write(C3dRenderState::AlphaBlendEnable, 1)
        );
        assert_eq!(
            program.writes[6],
            write(C3dRenderState::SourceBlend, contract.source_blend as u32)
        );
        assert_eq!(
            program.writes[7],
            write(
                C3dRenderState::DestinationBlend,
                contract.destination_blend as u32
            )
        );
    }

    #[test]
    fn indexed_branch_preserves_exact_write_order_and_values() {
        let program = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
            graphics_branch: C3dGraphicsBranch::Indexed,
            alpha_blend_enabled: false,
            filter_capability_available: true,
            cached_texture_handle: ROOT_MENU_C3D_INITIAL_TEXTURE_HANDLE_CACHE,
            cached_filter_request: ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        });

        assert_eq!(program.writes.len(), 22);
        let actual = program
            .writes
            .iter()
            .map(|write| (write.state as u32, write.value))
            .collect::<Vec<_>>();
        assert_eq!(
            actual.as_slice(),
            &[
                (9, 1),
                (27, 1),
                (27, 0),
                (41, 1),
                (3, 3),
                (33, 0),
                (19, 5),
                (20, 6),
                (4, 1),
                (7, 1),
                (14, 1),
                (23, 4),
                (22, 3),
                (21, 7),
                (8, 3),
                (26, 1),
                (29, 0),
                (2, 0),
                (28, 0),
                (34, 0x00ff_ffff),
                (17, 1),
                (18, 1),
            ]
        );
        assert_eq!(program.resulting_texture_handle_cache, 0);
        assert_eq!(program.resulting_filter_request_cache, 1);
    }

    #[test]
    fn alternate_branch_and_runtime_gates_are_explicit() {
        let program = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
            graphics_branch: C3dGraphicsBranch::Alternate,
            alpha_blend_enabled: true,
            filter_capability_available: true,
            cached_texture_handle: ROOT_MENU_C3D_INITIAL_TEXTURE_HANDLE_CACHE,
            cached_filter_request: ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
        });
        assert_eq!(
            program.writes.len(),
            ROOT_MENU_C3D_RENDER_STATE_BASE_WRITE_COUNT
        );
        assert_eq!(program.writes[0].value, 2);
        assert_eq!(program.writes[2].value, 1);
        assert_eq!(program.writes[13].value, 2);
        assert_eq!(program.resulting_filter_request_cache, 2);

        let disturbed = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
            filter_capability_available: false,
            graphics_branch: C3dGraphicsBranch::Alternate,
            alpha_blend_enabled: true,
            cached_texture_handle: 99,
            cached_filter_request: 1,
        });
        assert_eq!(
            disturbed.writes.len(),
            ROOT_MENU_C3D_RENDER_STATE_MAX_WRITE_COUNT
        );
        assert_eq!(
            disturbed.writes[20],
            write(C3dRenderState::TextureHandle, 0)
        );
        assert_eq!(disturbed.writes[21], write(C3dRenderState::TextureMag, 1));
        assert_eq!(disturbed.writes[22], write(C3dRenderState::TextureMin, 1));
        assert_eq!(disturbed.resulting_texture_handle_cache, 0);
        assert_eq!(disturbed.resulting_filter_request_cache, 2);
    }

    #[test]
    fn available_filter_capability_submits_the_alternate_request() {
        let program = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
            graphics_branch: C3dGraphicsBranch::Alternate,
            alpha_blend_enabled: false,
            filter_capability_available: true,
            cached_texture_handle: 0,
            cached_filter_request: 1,
        });
        assert_eq!(program.writes.len(), 22);
        assert_eq!(program.writes[20], write(C3dRenderState::TextureMag, 2));
        assert_eq!(program.writes[21], write(C3dRenderState::TextureMin, 2));
    }

    #[test]
    fn repeated_initialization_reuses_both_resulting_caches() {
        for graphics_branch in [C3dGraphicsBranch::Indexed, C3dGraphicsBranch::Alternate] {
            let first = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
                graphics_branch,
                alpha_blend_enabled: false,
                filter_capability_available: true,
                cached_texture_handle: ROOT_MENU_C3D_INITIAL_TEXTURE_HANDLE_CACHE,
                cached_filter_request: ROOT_MENU_C3D_INITIAL_FILTER_REQUEST_CACHE,
            });
            let second = root_menu_c3d_render_state_program(RootMenuC3dRenderStateInputs {
                graphics_branch,
                alpha_blend_enabled: false,
                filter_capability_available: true,
                cached_texture_handle: first.resulting_texture_handle_cache,
                cached_filter_request: first.resulting_filter_request_cache,
            });

            assert_eq!(
                second.writes.len(),
                ROOT_MENU_C3D_RENDER_STATE_BASE_WRITE_COUNT
            );
            assert!(second.writes.iter().all(|write| !matches!(
                write.state,
                C3dRenderState::TextureHandle
                    | C3dRenderState::TextureMag
                    | C3dRenderState::TextureMin
            )));
        }
    }

    #[test]
    fn enum_discriminants_match_legacy_abi_numbers() {
        assert_eq!(C3dRenderState::TextureHandle as u32, 1);
        assert_eq!(C3dRenderState::ZEnable as u32, 7);
        assert_eq!(C3dRenderState::ZWriteEnable as u32, 14);
        assert_eq!(C3dRenderState::CullMode as u32, 22);
        assert_eq!(C3dRenderState::AlphaBlendEnable as u32, 27);
        assert_eq!(C3dRenderState::ColorKeyEnable as u32, 41);
    }
}
