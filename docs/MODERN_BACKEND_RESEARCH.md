# Modern backend research

Status: provisional architecture decision, researched 2026-10-20. This does not
replace the compatibility backend and does not authorize observable behavior to
drift from recovered contracts.

## Recommendation

Use three deliberately separate layers:

1. Keep `cmr-core` platform-neutral and retain `cmr-video`'s deterministic
   software backend as the compatibility oracle and headless test target.
2. Use **SDL3 for platform services**: native windows/events, raw keyboard and
   joystick sampling, and audio device/stream handling. Standardized gamepad
   mapping is an optional modern adapter, not a replacement for the recovered
   input contract.
3. Use **wgpu for the modern GPU renderer**, hosted in the SDL window through
   raw-window-handle. Keep SDL's GPU API as a fallback candidate until a small
   representative benchmark has been run; do not build the game on SDL's 2D
   renderer.

This is intentionally “SDL3 plus wgpu,” not “SDL for everything.” It gives the
project SDL's unusually complete game-platform layer while keeping the renderer
Rust-native, explicit, and suitable for the later 3D reconstruction.

## Why this fits this game

The compatibility renderer needs exact 8-bit index-zero transparency and
palette changes. A modern GPU path can represent each source as an `R8Uint`
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

| Candidate | Strength | Reason not selected as the default |
| --- | --- | --- |
| SDL3 platform + SDL_GPU | One C dependency provides the whole host layer; SDL_GPU supports modern Vulkan/D3D12/Metal-style rendering and universally sampleable `R8_UNORM`. | Attractive fallback, but its D3D12 backend requires Windows 10+, DX12 feature level 11_0, and Resource Binding Tier 2; Vulkan can also be used on Windows and is the Linux route. It lacks wgpu's secondary GL fallback, and current high-level Rust SDL3 coverage is still evolving. [SDL GPU requirements](https://wiki.libsdl.org/SDL3/CategoryGPU), [texture formats](https://wiki.libsdl.org/SDL3/SDL_GPUTextureFormat). |
| winit + wgpu + cpal + gilrs | Mostly Rust-native, with explicit components and strong wgpu integration. | More independent lifecycle, hotplug, packaging, and platform abstractions to reconcile. winit intentionally covers only windows and input events, not rendering or the rest of a game platform. [winit scope](https://github.com/rust-windowing/winit/blob/master/FEATURES.md). |
| SDL3 + bgfx | Mature multi-API renderer with threaded submission and broad backend selection. | Adds a large C++ build and separate shader toolchain to a Rust project while SDL is still needed for input/audio/windowing. bgfx explicitly positions itself as a renderer brought into another framework. [bgfx overview](https://bkaradzic.github.io/bgfx/), [shader tools](https://bkaradzic.github.io/bgfx/tools.html). |
| SDL 2D renderer | Small API and adequate for simple textured quads. | SDL documents it as a 2D API and directs advanced/3D work to lower-level graphics APIs or engines; it would become a dead end once cars, tracks, lighting, and effects are reconstructed. [SDL render module](https://docs.rs/sdl3/latest/sdl3/render/). |

## Performance and compatibility policy

No documentation comparison proves which abstraction is fastest for this game.
Backend overhead should be small relative to rendering, but that remains an
inference until measured. Before locking the modern renderer, first require both
implementations to match the software oracle for index-zero transparency,
clipping, draw order, palette changes, resize/fullscreen transitions, errors,
and device recreation. Compare final RGBA pixels with an explicit swapchain
color-space policy as well as the pre-palette index/blit trace. Only conforming
implementations proceed to performance measurement on at least:

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

Future platform crates should depend inward on recovered traits:

```text
cmr-core contracts
      ^
      |---- cmr-video-software  (oracle/headless)
      |---- cmr-platform-sdl3   (window, events, input, audio)
      `---- cmr-video-wgpu      (modern GPU renderer)
```

Do not expose SDL or wgpu handles in game-domain modules. Palette-index surfaces,
blit plans, input samples, audio buffers, clocks, and presentation commands stay
backend-neutral. This lets the SDL_GPU spike implement the same traits and makes
the final renderer choice reversible.

## Decision gates

The provisional recommendation becomes final only when:

- a pinned SDL3 Rust binding builds and opens a window on CI or controlled
  Windows and Linux hosts;
- wgpu displays a trace- and pixel-equivalent indexed/palette compatibility
  frame on explicitly selected DX12 and Vulkan backends;
- SDL input/gamepad/audio adapters pass shared contract tests;
- the wgpu/SDL_GPU comparison above has evidence rather than estimates;
- packaging succeeds without requiring users to install development libraries.

