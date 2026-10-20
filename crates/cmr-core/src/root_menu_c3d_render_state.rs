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
    let (shade_mode, texture_map_blend, branch_filter) = match inputs.graphics_branch {
        C3dGraphicsBranch::Indexed => (1, 7, 1),
        C3dGraphicsBranch::Alternate => (2, 2, 2),
    };
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
        write(State::SourceBlend, 5),
        write(State::DestinationBlend, 6),
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
    if inputs.cached_filter_request != branch_filter {
        let submitted_filter = if inputs.filter_capability_available {
            branch_filter
        } else {
            1
        };
        writes.push(write(State::TextureMag, submitted_filter));
        writes.push(write(State::TextureMin, submitted_filter));
    }
    RootMenuC3dRenderStateProgram {
        writes,
        resulting_texture_handle_cache: 0,
        resulting_filter_request_cache: branch_filter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
