# Modern backend research

Status: Phase 4A presentation conformance implemented 2026-10-24; native C3D
comparison remains provisional. This does not replace the compatibility backend
and does not authorize observable behavior to drift from recovered contracts.

## Phase 4A result

The source-owned `cmr-gpu-contract` contract now validates an indexed frame and
expands it to the canonical opaque RGBA8 oracle. Both candidates implement that
same contract offscreen:

- `cmr-video-wgpu` uses an `R8Uint` index texture, a 256-entry `Rgba8Uint`
  palette texture, integer shader loads, and an `Rgba8Unorm` target;
- `cmr-video-sdl-gpu` implements the same pipeline through SDL_GPU's Vulkan
  backend, with a narrow private `sdl3::sys` bridge for capabilities and
  readback calls missing from the safe wrapper;
- `cmr-gpu-conformance` checks the contract oracle against the established
  `cmr-video::SoftwarePresentedFrame`, then requires exact candidate readback
  equality for an asymmetric 257x2 fixture covering every index and a
  palette-only mutation.

Local Linux runs passed on both an NVIDIA RTX 5090 and an AMD Radeon 610M/RADV.
The runner deliberately selects no winner: this proves indexed-palette
presentation correctness, not native C3D rendering performance. SDL_GPU's
current source-shader build uses an external `glslangValidator` and is
Vulkan-only; a reproducible shader package and its DX12 shader path remain open.
The final native backend choice is gated on both candidates consuming identical
raw recovered C3D draw packets and rendering a caller-selected imported stage.

## Recommendation

Use four deliberately separate layers:

1. Keep `cmr-core` platform-neutral and retain `cmr-video`'s deterministic
   software backend as the compatibility oracle and headless test target.
2. Use **SDL3 for platform services**: native windows/events, raw keyboard and
   joystick sampling, and audio device/stream handling. Standardized gamepad
   mapping is an optional modern adapter, not a replacement for the recovered
   input contract.
3. For the first native compatibility executable, present the completed
   software-rendered indexed framebuffer through **SDL_Renderer**. This keeps
   recovered raster behavior in the oracle and gives Windows and Linux a broad
   D3D/OpenGL/Vulkan/software presentation fallback without prematurely
   reconstructing the 3D pipeline on a GPU API.
4. For native 3D, keep **wgpu as the provisional default** and **SDL_GPU as the
   mandatory comparison**. Select between them only after C3D contracts exist
   and both pass the same correctness, recovery, packaging, and performance
   bake-off.

This is a staged choice, not “SDL for everything.” SDL3 minimizes platform,
peripheral, and audio integration risk. SDL_Renderer gets the compatibility
build on screen with the least semantic risk. The eventual GPU-native renderer
remains reversible until measurements support a lock-in.

## Why this fits this game

The compatibility renderer needs exact 8-bit index-zero transparency and
palette changes. Initially, those rules remain in the software backend; the
presenter consumes a completed 640x480 indexed frame plus its palette. Expanding
and uploading that small frame avoids making SDL_Renderer reproduce keyed-blit
semantics. SDL also supports logical presentation and selectable accelerated or
software render drivers. SDL 3.4 adds palettized texture support, but the first
implementation should retain a plain RGBA upload fallback so binding or driver
support cannot block compatibility. See SDL's
[logical presentation API](https://wiki.libsdl.org/SDL3/SDL_SetRenderLogicalPresentation),
[renderer-driver selection](https://wiki.libsdl.org/SDL3/SDL_HINT_RENDER_DRIVER),
and [texture palette API](https://wiki.libsdl.org/SDL3/SDL_SetTexturePalette).

A later GPU-native path can represent each indexed source as an `R8Uint`
texture and perform an integer texel load into a 256-entry RGBA palette. That
keeps indices exact and avoids CPU re-expansion/re-upload when the active
palette changes. wgpu exposes `R8Uint` without numeric conversion and supports
DX12 on Windows, Vulkan on Windows/Linux, and a secondary GL/GLES path. Its
current MSRV is 1.87, below this workspace's Rust 1.88 floor. See the official
[wgpu backend matrix](https://github.com/gfx-rs/wgpu#supported-platforms),
[`R8Uint` format definition](https://docs.rs/wgpu/30.0.1/wgpu/enum.TextureFormat.html),
and [wgpu 30.0.1 manifest](https://docs.rs/crate/wgpu/30.0.1/source/Cargo.toml).

SDL3 covers the parts that a graphics API does not: its supported platform list
includes Windows and Linux, and its gamepad layer ships mappings for common
devices and supports hotplugging. Logical devices opened as the default follow
device migration; streams provide buffering, conversion, and resampling, while
the logical device mixes bound streams. See SDL's
[platform overview](https://wiki.libsdl.org/SDL3/FrontPage),
[gamepad API](https://wiki.libsdl.org/SDL3/CategoryGamepad), and
[audio API](https://wiki.libsdl.org/SDL3/CategoryAudio).

The evaluated Rust wrapper is `sdl3` 0.20.0. It exposes a
`raw-window-handle` integration intended for wgpu, but its own documentation
warns that some SDL3 features remain only partially migrated. The adapter must
therefore remain narrow and version-pinned. For a missing call, use the exact
matching `sdl3::sys` re-export instead of independently resolving bindings, and
do not let FFI types escape the platform crate. See the
[sdl3 0.20.0 feature status](https://docs.rs/sdl3/0.20.0/sdl3/) and its
[matching sys re-export](https://docs.rs/sdl3/0.20.0/sdl3/sys/index.html).

## Alternatives considered

| Candidate | Strength | Role / disposition |
| --- | --- | --- |
| SDL_Renderer presentation | Broad drivers, logical sizing, a software fallback, and little code between the exact software rasterizer and a native window. | Selected for the first compatibility executable, not for native C3D rendering. It presents completed frames and must not become a second implementation of recovered raster rules. [SDL render API](https://wiki.libsdl.org/SDL3/CategoryRender). |
| SDL3 platform + SDL_GPU | One C dependency provides the whole host layer; SDL_GPU supports modern Vulkan/D3D12/Metal-style rendering and sampleable single-channel formats. | Mandatory native-3D contender, but its D3D12 backend requires Windows 10+, DX12 feature level 11_0, and normally Resource Binding Tier 2; Vulkan can also be used on Windows and is the Linux route. It lacks wgpu's secondary GL fallback, and current high-level Rust SDL3 coverage is still evolving. [SDL GPU requirements](https://wiki.libsdl.org/SDL3/CategoryGPU), [texture formats](https://wiki.libsdl.org/SDL3/SDL_GPUTextureFormat). |
| winit + wgpu + cpal + gilrs | Mostly Rust-native, with explicit components and strong wgpu integration. | More independent lifecycle, hotplug, packaging, and platform abstractions to reconcile. winit intentionally covers only windows and input events, not rendering or the rest of a game platform. [winit scope](https://github.com/rust-windowing/winit/blob/master/FEATURES.md). |
| SDL3 + bgfx | Mature multi-API renderer with threaded submission and broad backend selection. | Adds a large C++ build and separate shader toolchain to a Rust project while SDL is still needed for input/audio/windowing. bgfx explicitly positions itself as a renderer brought into another framework. [bgfx overview](https://bkaradzic.github.io/bgfx/), [shader tools](https://bkaradzic.github.io/bgfx/tools.html). |

## Performance and compatibility policy

No authoritative comparison proves which GPU abstraction is fastest for this
game. Backend overhead should be small relative to rendering, but that remains
an inference until measured. SDL_Renderer is therefore chosen for compatibility
by simplicity and driver reach, not a performance claim. Before locking the
native 3D renderer, first require the wgpu and SDL_GPU implementations to match
the software oracle for index-zero transparency, clipping, draw order, palette
changes, resize/fullscreen transitions, errors, and device recreation. Compare
final RGBA pixels with an explicit swapchain color-space policy as well as the
pre-palette index/blit trace. Only conforming implementations proceed to
performance measurement on at least:

- Windows with DX12 explicitly selected for each candidate, plus a same-machine
  Vulkan comparison where both support it;
- Linux with Vulkan explicitly selected under both X11 and Wayland;
- one integrated GPU and one discrete GPU where available.

The initial spike must exercise palette updates, hundreds of keyed indexed
sprites, resize/fullscreen transitions, frame pacing, device loss, and
startup/package size. Add dynamic 3D vertex/index traffic only after C3D
geometry, materials, and submission contracts are recovered; do not benchmark
invented rally-scene traffic. Record median, 95th, and 99th-percentile CPU/GPU
frame time rather than average FPS alone. Test automatic backend selection
separately from controlled performance comparisons.

Compatibility takes precedence over a marginal benchmark win. The selected GPU
backend must pass shared renderer contracts derived from the software oracle.
When neither GPU route initializes, the software backend should remain usable
for diagnostics/headless verification; whether it becomes an end-user windowed
fallback is a later product decision.

## Integration boundary

Future platform crates should depend inward on recovered traits. The first
native slice ends at the presenter; native GPU rasterization comes later:

```text
cmr-core contracts
      ^
      |---- cmr-video-software  (oracle/headless)
      |---- cmr-platform-sdl3   (window, events, input, audio)
      |---- cmr-present-sdl3    (compatibility framebuffer presentation)
      |---- cmr-video-wgpu      (provisional native GPU renderer)
      `---- cmr-video-sdl-gpu   (native GPU comparison)
```

Do not expose SDL or wgpu handles in game-domain modules. Palette-index surfaces,
blit plans, input samples, audio buffers, clocks, and presentation commands stay
backend-neutral. This lets the SDL_GPU spike implement the same traits and makes
the final renderer choice reversible.

## Decision gates

The provisional recommendation becomes final only when:

- a pinned SDL3 binding builds and opens a window on controlled Windows and
  Linux hosts;
- SDL_Renderer presents an oracle-produced compatibility frame with exact RGBA
  expansion, nearest/integer scaling policy, and stable resize/fullscreen
  behavior;
- wgpu and SDL_GPU display trace- and pixel-equivalent indexed/palette fixtures
  on explicitly selected DX12 and Vulkan backends;
- SDL input/gamepad/audio adapters pass shared contract tests;
- the wgpu/SDL_GPU comparison above has evidence rather than estimates;
- packaging succeeds without requiring users to install development libraries.

