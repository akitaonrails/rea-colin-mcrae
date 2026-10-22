//! SDL3 host adapter for completed compatibility frames.
//!
//! SDL owns the native window, event queue, and opened gamepad handles. It
//! does not participate in recovered rasterization: callers supply one
//! completed indexed software frame, which is expanded to opaque RGBA8 before
//! upload to an SDL streaming texture.

pub mod audio;
pub mod input;

pub use audio::{AudioHost, SharedSoftwareMixer};
pub use input::{
    AXIS_DEAD_ZONE, FrontendAction, FrontendInputMapper, KeyboardVkState, action_for_button,
    action_for_key, axis_action, sdl_keycode_to_vk,
};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::time::{Duration, Instant};

use cmr_core::frontend::FrontendInput;
use cmr_core::host_lifecycle::{HostLifecycle, PlatformLifecycleEvent};
use cmr_core::present_scaling::{DEFAULT_SCALING, PresentationScaling};
use cmr_core::root_menu_interactive::RootInteraction;
use cmr_core::root_menu_visual::RootRenderInput;
use cmr_core::window::{WindowEffect, WindowMode};
use cmr_video::{SoftwarePresentedFrame, SoftwareRgba8Error, SoftwareRgba8Frame};
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::pixels::PixelFormat;
use sdl3::render::{Canvas, ScaleMode, Texture};
use sdl3::video::Window;

/// Target frame interval for the interactive loop's event wait (~60 Hz).
const INTERACTIVE_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Map the project's backend-neutral presentation scaling
/// ([`cmr_core::present_scaling`]) onto the SDL logical-presentation mode. This
/// is how the SDL backend implements that contract; a later GPU backend instead
/// computes its viewport from `cmr_core::present_scaling::fit_viewport`. The two
/// must agree, which is why the geometry is defined once in `cmr-core`.
fn logical_presentation(
    scaling: PresentationScaling,
) -> sdl3::sys::render::SDL_RendererLogicalPresentation {
    match scaling {
        PresentationScaling::Stretch => sdl3::sys::render::SDL_LOGICAL_PRESENTATION_STRETCH,
        PresentationScaling::Letterbox => sdl3::sys::render::SDL_LOGICAL_PRESENTATION_LETTERBOX,
        PresentationScaling::IntegerScale => {
            sdl3::sys::render::SDL_LOGICAL_PRESENTATION_INTEGER_SCALE
        }
    }
}

/// Platform-neutral events relevant to the first native host loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEvent {
    QuitRequested,
    EscapePressed,
    GamepadAdded(u32),
    GamepadRemoved(u32),
    RedrawRequested,
    RendererReset,
    FocusGained,
    FocusLost,
    Ignored,
}

impl HostEvent {
    /// The backend-neutral [`PlatformLifecycleEvent`] this event drives, if any.
    /// This is how the SDL adapter feeds the recovered host lifecycle
    /// ([`cmr_core::host_lifecycle`]): a close/quit shuts down, focus changes
    /// pause and resume. Input and redraw events are not lifecycle events.
    pub fn as_lifecycle(self) -> Option<PlatformLifecycleEvent> {
        match self {
            HostEvent::QuitRequested => Some(PlatformLifecycleEvent::CloseRequested),
            HostEvent::FocusGained => Some(PlatformLifecycleEvent::FocusGained),
            HostEvent::FocusLost => Some(PlatformLifecycleEvent::FocusLost),
            _ => None,
        }
    }
}

/// Testable lifecycle state mirrored by the SDL event adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostEventState {
    exit_requested: bool,
    focused: bool,
    connected_gamepads: BTreeSet<u32>,
}

impl Default for HostEventState {
    fn default() -> Self {
        Self {
            exit_requested: false,
            // The window starts focused once SDL creates it; a later FocusLost
            // parks the loop and a FocusGained resumes it.
            focused: true,
            connected_gamepads: BTreeSet::new(),
        }
    }
}

impl HostEventState {
    #[must_use]
    pub fn with_connected_gamepads(ids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            connected_gamepads: ids.into_iter().collect(),
            ..Self::default()
        }
    }

    pub fn apply(&mut self, event: HostEvent) {
        match event {
            HostEvent::QuitRequested | HostEvent::EscapePressed => self.exit_requested = true,
            HostEvent::GamepadAdded(id) => {
                self.connected_gamepads.insert(id);
            }
            HostEvent::GamepadRemoved(id) => {
                self.connected_gamepads.remove(&id);
            }
            HostEvent::FocusGained => self.focused = true,
            HostEvent::FocusLost => self.focused = false,
            HostEvent::RedrawRequested | HostEvent::RendererReset | HostEvent::Ignored => {}
        }
    }

    /// Whether the window currently holds input focus.
    pub fn focused(&self) -> bool {
        self.focused
    }

    #[must_use]
    pub const fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    #[must_use]
    pub fn connected_gamepads(&self) -> &BTreeSet<u32> {
        &self.connected_gamepads
    }
}

/// Native presentation failures kept outside compatibility crates.
#[derive(Debug)]
pub enum SdlHostError {
    FrameConversion(SoftwareRgba8Error),
    InvalidUploadLayout {
        source_pitch: usize,
        destination_pitch: usize,
        height: usize,
    },
    Sdl {
        operation: &'static str,
        detail: String,
    },
}

impl SdlHostError {
    fn sdl(operation: &'static str, source: impl fmt::Display) -> Self {
        Self::Sdl {
            operation,
            detail: source.to_string(),
        }
    }
}

impl fmt::Display for SdlHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameConversion(source) => source.fmt(formatter),
            Self::InvalidUploadLayout {
                source_pitch,
                destination_pitch,
                height,
            } => write!(
                formatter,
                "invalid SDL upload layout: source pitch {source_pitch}, destination pitch {destination_pitch}, height {height}"
            ),
            Self::Sdl { operation, detail } => {
                write!(formatter, "SDL {operation} failed: {detail}")
            }
        }
    }
}

impl std::error::Error for SdlHostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::FrameConversion(source) => Some(source),
            Self::InvalidUploadLayout { .. } | Self::Sdl { .. } => None,
        }
    }
}

/// Opens a resizable SDL3 window and presents a completed software frame until
/// the user closes the window or presses Escape.
///
/// Connected SDL gamepads are opened at startup and tracked across hotplug
/// events so later input mapping can consume stable live handles.
pub fn present_software_frame(frame: &SoftwarePresentedFrame) -> Result<(), SdlHostError> {
    present_software_frame_impl(frame, true)
}

/// Opens the SDL presenter, uploads and presents one frame, then returns.
///
/// This bounded entry point exists for display-server and dummy-video smoke
/// tests. The normal application uses [`present_software_frame`] so its event
/// and gamepad lifecycle remains active.
pub fn present_software_frame_once(frame: &SoftwarePresentedFrame) -> Result<(), SdlHostError> {
    present_software_frame_impl(frame, false)
}

fn present_software_frame_impl(
    frame: &SoftwarePresentedFrame,
    wait_for_exit: bool,
) -> Result<(), SdlHostError> {
    let rgba = frame
        .expand_rgba8()
        .map_err(SdlHostError::FrameConversion)?;
    let extent = rgba.extent();

    let sdl = sdl3::init().map_err(|error| SdlHostError::sdl("initialization", error))?;
    let video = sdl
        .video()
        .map_err(|error| SdlHostError::sdl("video initialization", error))?;
    let gamepad = sdl
        .gamepad()
        .map_err(|error| SdlHostError::sdl("gamepad initialization", error))?;
    let window = video
        .window(
            "Colin McRae Rally — reconstruction",
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| SdlHostError::sdl("window creation", error))?;
    let mut canvas = window.into_canvas();
    canvas
        .set_logical_size(
            u32::from(extent.width),
            u32::from(extent.height),
            logical_presentation(DEFAULT_SCALING),
        )
        .map_err(|error| SdlHostError::sdl("logical presentation setup", error))?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(
            PixelFormat::RGBA32,
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .map_err(|error| SdlHostError::sdl("streaming texture creation", error))?;
    texture.set_scale_mode(ScaleMode::Nearest);
    upload_rgba_frame(&mut texture, &rgba)?;
    canvas.clear();
    canvas
        .copy(&texture, None, None)
        .map_err(|error| SdlHostError::sdl("frame copy", error))?;
    if !canvas.present() {
        return Err(SdlHostError::sdl("frame presentation", sdl3::get_error()));
    }
    if !wait_for_exit {
        return Ok(());
    }

    let mut open_gamepads = BTreeMap::new();
    for id in gamepad
        .gamepads()
        .map_err(|error| SdlHostError::sdl("gamepad enumeration", error))?
    {
        if let Ok(handle) = gamepad.open(id) {
            open_gamepads.insert(id.raw(), handle);
        }
    }
    let mut state = HostEventState::with_connected_gamepads(open_gamepads.keys().copied());
    let mut events = sdl
        .event_pump()
        .map_err(|error| SdlHostError::sdl("event pump creation", error))?;

    while !state.exit_requested() {
        let event = events.wait_event();
        let host_event = map_event(&event);
        let apply_event = match host_event {
            HostEvent::GamepadAdded(id) => {
                if let Ok(handle) = gamepad.open(id.into()) {
                    open_gamepads.insert(id, handle);
                    true
                } else {
                    false
                }
            }
            HostEvent::GamepadRemoved(id) => {
                open_gamepads.remove(&id);
                true
            }
            HostEvent::RendererReset => {
                texture = texture_creator
                    .create_texture_streaming(
                        PixelFormat::RGBA32,
                        u32::from(extent.width),
                        u32::from(extent.height),
                    )
                    .map_err(|error| SdlHostError::sdl("streaming texture recreation", error))?;
                texture.set_scale_mode(ScaleMode::Nearest);
                upload_rgba_frame(&mut texture, &rgba)?;
                redraw_present(&mut canvas, &texture)?;
                true
            }
            HostEvent::RedrawRequested => {
                redraw_present(&mut canvas, &texture)?;
                true
            }
            _ => true,
        };
        if apply_event {
            state.apply(host_event);
        }
    }

    Ok(())
}

fn redraw_present(canvas: &mut Canvas<Window>, texture: &Texture<'_>) -> Result<(), SdlHostError> {
    canvas.clear();
    canvas
        .copy(texture, None, None)
        .map_err(|error| SdlHostError::sdl("frame redraw", error))?;
    if !canvas.present() {
        return Err(SdlHostError::sdl("frame redraw", sdl3::get_error()));
    }
    Ok(())
}

fn upload_rgba_frame(
    texture: &mut Texture<'_>,
    frame: &SoftwareRgba8Frame,
) -> Result<(), SdlHostError> {
    let mut upload_result = Ok(());
    texture
        .with_lock(None, |destination, destination_pitch| {
            upload_result = copy_packed_rows(
                frame.pixels(),
                frame.row_pitch(),
                destination,
                destination_pitch,
                usize::from(frame.extent().height),
            );
        })
        .map_err(|error| SdlHostError::sdl("streaming texture lock", error))?;
    upload_result
}

fn map_event(event: &Event) -> HostEvent {
    match event {
        Event::Quit { .. } => HostEvent::QuitRequested,
        Event::KeyDown {
            keycode: Some(Keycode::Escape),
            ..
        } => HostEvent::EscapePressed,
        Event::GamepadAdded { which, .. } => HostEvent::GamepadAdded(which.raw()),
        Event::GamepadRemoved { which, .. } => HostEvent::GamepadRemoved(which.raw()),
        Event::Window {
            win_event: WindowEvent::FocusGained,
            ..
        } => HostEvent::FocusGained,
        Event::Window {
            win_event: WindowEvent::FocusLost | WindowEvent::Minimized,
            ..
        } => HostEvent::FocusLost,
        Event::Window {
            win_event:
                WindowEvent::Exposed
                | WindowEvent::Resized(_, _)
                | WindowEvent::PixelSizeChanged(_, _)
                | WindowEvent::Restored,
            ..
        }
        | Event::RenderTargetsReset { .. } => HostEvent::RedrawRequested,
        Event::RenderDeviceReset { .. } => HostEvent::RendererReset,
        _ => HostEvent::Ignored,
    }
}

fn copy_packed_rows(
    source: &[u8],
    source_pitch: usize,
    destination: &mut [u8],
    destination_pitch: usize,
    height: usize,
) -> Result<(), SdlHostError> {
    let source_len = source_pitch.checked_mul(height);
    let destination_len = destination_pitch.checked_mul(height);
    if destination_pitch < source_pitch
        || source_len != Some(source.len())
        || destination_len.is_none_or(|required| destination.len() < required)
    {
        return Err(SdlHostError::InvalidUploadLayout {
            source_pitch,
            destination_pitch,
            height,
        });
    }
    for row in 0..height {
        let source_start = row * source_pitch;
        let destination_start = row * destination_pitch;
        destination[destination_start..destination_start + source_pitch]
            .copy_from_slice(&source[source_start..source_start + source_pitch]);
    }
    Ok(())
}

/// A failure while running the interactive root-menu loop.
#[derive(Debug)]
pub enum InteractiveRootError<E> {
    /// A host (SDL) presentation or window-system failure.
    Host(SdlHostError),
    /// A failure returned by the caller's frame-render callback.
    Render(E),
}

impl<E: fmt::Display> fmt::Display for InteractiveRootError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(source) => source.fmt(formatter),
            Self::Render(source) => write!(formatter, "interactive render failed: {source}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for InteractiveRootError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Host(source) => Some(source),
            Self::Render(source) => Some(source),
        }
    }
}

fn elapsed_ms(clock: Instant) -> u32 {
    clock.elapsed().as_millis() as u32
}

fn rendered_rgba<E>(
    frame: SoftwarePresentedFrame,
) -> Result<SoftwareRgba8Frame, InteractiveRootError<E>> {
    frame
        .expand_rgba8()
        .map_err(|error| InteractiveRootError::Host(SdlHostError::FrameConversion(error)))
}

/// Runs the interactive root-menu loop until the user exits.
///
/// SDL owns the window, event queue, gamepads, and presentation. Navigation
/// and exit come from the recovered [`RootInteraction`]; the `render` callback
/// turns each distinct recovered [`RootRenderInput`] into a completed software
/// frame (typically by driving the compatibility session). The frame is only
/// re-rendered when the controller's render input actually changes.
///
/// `Escape` and the window close button exit, as in the static presenter. The
/// controller supplies a monotonic millisecond clock so its recovered idle and
/// attract timing can elapse.
pub fn run_interactive_root_frame<E>(
    mut interaction: RootInteraction,
    save_allowed: bool,
    mut render: impl FnMut(&RootRenderInput) -> Result<SoftwarePresentedFrame, E>,
) -> Result<(), InteractiveRootError<E>> {
    let clock = Instant::now();

    // Settle the recovered controller into the interactive root screen and
    // render its first frame before opening a window.
    let mut current_input = loop {
        let tick = interaction.tick(FrontendInput::default(), elapsed_ms(clock), save_allowed);
        if tick.exit_requested {
            return Ok(());
        }
        if let Some(input) = tick.render_input {
            break input;
        }
    };
    let mut rgba = rendered_rgba(render(&current_input).map_err(InteractiveRootError::Render)?)?;
    let extent = rgba.extent();

    let host = InteractiveRootError::Host;
    let sdl = sdl3::init().map_err(|error| host(SdlHostError::sdl("initialization", error)))?;
    let video = sdl
        .video()
        .map_err(|error| host(SdlHostError::sdl("video initialization", error)))?;
    let gamepad = sdl
        .gamepad()
        .map_err(|error| host(SdlHostError::sdl("gamepad initialization", error)))?;
    let window = video
        .window(
            "Colin McRae Rally — reconstruction",
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| host(SdlHostError::sdl("window creation", error)))?;
    let mut canvas = window.into_canvas();
    canvas
        .set_logical_size(
            u32::from(extent.width),
            u32::from(extent.height),
            logical_presentation(DEFAULT_SCALING),
        )
        .map_err(|error| host(SdlHostError::sdl("logical presentation setup", error)))?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(
            PixelFormat::RGBA32,
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .map_err(|error| host(SdlHostError::sdl("streaming texture creation", error)))?;
    texture.set_scale_mode(ScaleMode::Nearest);
    upload_rgba_frame(&mut texture, &rgba).map_err(InteractiveRootError::Host)?;
    redraw_present(&mut canvas, &texture).map_err(InteractiveRootError::Host)?;

    let mut open_gamepads = BTreeMap::new();
    for id in gamepad
        .gamepads()
        .map_err(|error| host(SdlHostError::sdl("gamepad enumeration", error)))?
    {
        if let Ok(handle) = gamepad.open(id) {
            open_gamepads.insert(id.raw(), handle);
        }
    }
    let mut host_state = HostEventState::with_connected_gamepads(open_gamepads.keys().copied());
    // The window is created focused; drive focus/pause/restore/shutdown through
    // the recovered host lifecycle (B12) as SDL reports focus and close events.
    let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
    lifecycle.apply(PlatformLifecycleEvent::FocusGained);
    let mut mapper = FrontendInputMapper::new();
    let mut events = sdl
        .event_pump()
        .map_err(|error| host(SdlHostError::sdl("event pump creation", error)))?;

    while !host_state.exit_requested() {
        let mut pending = Vec::new();
        if let Some(event) = events.wait_event_timeout(INTERACTIVE_FRAME_INTERVAL) {
            pending.push(event);
            pending.extend(events.poll_iter());
        }
        let mut host_events = Vec::with_capacity(pending.len());
        for event in pending {
            feed_input_event(&event, &mut mapper);
            let host_event = map_event(&event);
            host_events.push(host_event);
            match host_event {
                HostEvent::GamepadAdded(id) => {
                    if let Ok(handle) = gamepad.open(id.into()) {
                        open_gamepads.insert(id, handle);
                        host_state.apply(HostEvent::GamepadAdded(id));
                    }
                }
                HostEvent::GamepadRemoved(id) => {
                    open_gamepads.remove(&id);
                    host_state.apply(HostEvent::GamepadRemoved(id));
                }
                HostEvent::RendererReset => {
                    texture = texture_creator
                        .create_texture_streaming(
                            PixelFormat::RGBA32,
                            u32::from(extent.width),
                            u32::from(extent.height),
                        )
                        .map_err(|error| {
                            host(SdlHostError::sdl("streaming texture recreation", error))
                        })?;
                    texture.set_scale_mode(ScaleMode::Nearest);
                    upload_rgba_frame(&mut texture, &rgba).map_err(InteractiveRootError::Host)?;
                    redraw_present(&mut canvas, &texture).map_err(InteractiveRootError::Host)?;
                }
                HostEvent::RedrawRequested => {
                    redraw_present(&mut canvas, &texture).map_err(InteractiveRootError::Host)?;
                }
                other => host_state.apply(other),
            }
        }
        let decision = advance_root_lifecycle(&host_events, &mut lifecycle);
        if host_state.exit_requested() || decision.quit {
            break;
        }
        if decision.restore_surfaces {
            redraw_present(&mut canvas, &texture).map_err(InteractiveRootError::Host)?;
        }
        if !decision.active {
            // Unfocused: pause ticking and rendering; block on the next event.
            continue;
        }

        let tick = interaction.tick(mapper.poll(), elapsed_ms(clock), save_allowed);
        if tick.exit_requested {
            break;
        }
        if let Some(render_input) = tick.render_input
            && render_input != current_input
        {
            current_input = render_input;
            rgba = rendered_rgba(render(&current_input).map_err(InteractiveRootError::Render)?)?;
            upload_rgba_frame(&mut texture, &rgba).map_err(InteractiveRootError::Host)?;
            redraw_present(&mut canvas, &texture).map_err(InteractiveRootError::Host)?;
        }
    }

    Ok(())
}

/// One windowed game-session frame, returned by the `step` callback of
/// [`run_windowed_game_session`].
pub struct WindowedGameFrame {
    /// The software frame to present this tick (e.g. the stage composed from a
    /// follow camera), or `None` to clear to black when the active render route
    /// composes no world scene.
    pub present: Option<SoftwarePresentedFrame>,
    /// Whether the session has reached the terminal stopped state.
    pub stopped: bool,
}

/// Runs the windowed game session: drives the recovered top-level FSM with live
/// keyboard input and presents the frame the recovered render route selected,
/// until the session stops or the window closes.
///
/// SDL owns the window, keyboard, and presentation; the recovered game logic is
/// entirely in the `step` callback, which is handed the live held-key state (the
/// [`KeyboardVkState`] the loop maintains from key events through
/// [`sdl_keycode_to_vk`]) and returns that tick's [`WindowedGameFrame`]: the
/// software frame to present (the caller composes it per the recovered render
/// route, e.g. the stage from a follow camera) or `None` to clear. `initial`
/// sizes the window and is presented first. `Escape` is left to the game (its
/// recovered pause binding, VK `0x1b`); the window close button and a quit event
/// exit. Focus loss pauses the stepping, as the recovered host loop does.
pub fn run_windowed_game_session<F>(
    initial: &SoftwarePresentedFrame,
    mut step: F,
) -> Result<(), SdlHostError>
where
    F: FnMut(&KeyboardVkState) -> WindowedGameFrame,
{
    let rgba = initial.expand_rgba8().map_err(SdlHostError::FrameConversion)?;
    let extent = rgba.extent();

    let sdl = sdl3::init().map_err(|error| SdlHostError::sdl("initialization", error))?;
    let video = sdl
        .video()
        .map_err(|error| SdlHostError::sdl("video initialization", error))?;
    let window = video
        .window(
            "Colin McRae Rally — reconstruction",
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .position_centered()
        .resizable()
        .build()
        .map_err(|error| SdlHostError::sdl("window creation", error))?;
    let mut canvas = window.into_canvas();
    canvas
        .set_logical_size(
            u32::from(extent.width),
            u32::from(extent.height),
            logical_presentation(DEFAULT_SCALING),
        )
        .map_err(|error| SdlHostError::sdl("logical presentation setup", error))?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(
            PixelFormat::RGBA32,
            u32::from(extent.width),
            u32::from(extent.height),
        )
        .map_err(|error| SdlHostError::sdl("streaming texture creation", error))?;
    texture.set_scale_mode(ScaleMode::Nearest);
    upload_rgba_frame(&mut texture, &rgba)?;
    redraw_present(&mut canvas, &texture)?;

    let mut events = sdl
        .event_pump()
        .map_err(|error| SdlHostError::sdl("event pump creation", error))?;
    let mut keys = KeyboardVkState::new();
    // Drive focus/pause/restore/shutdown through the recovered host lifecycle
    // (B12), seeded focused at window creation, exactly as the interactive root
    // loop does.
    let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
    lifecycle.apply(PlatformLifecycleEvent::FocusGained);

    loop {
        let mut host_events = Vec::new();
        if let Some(event) = events.wait_event_timeout(INTERACTIVE_FRAME_INTERVAL) {
            for event in std::iter::once(event).chain(events.poll_iter()) {
                match event {
                    Event::KeyDown {
                        keycode: Some(code),
                        ..
                    } => keys.apply_key(code, true),
                    Event::KeyUp {
                        keycode: Some(code),
                        ..
                    } => keys.apply_key(code, false),
                    _ => {}
                }
                host_events.push(map_event(&event));
            }
        }
        let decision = advance_root_lifecycle(&host_events, &mut lifecycle);
        if decision.quit {
            break;
        }
        if decision.restore_surfaces {
            redraw_present(&mut canvas, &texture)?;
        }
        if !decision.active {
            // Unfocused: pause stepping and presenting; block on the next event.
            continue;
        }

        let frame = step(&keys);
        if frame.stopped {
            break;
        }
        match frame.present {
            Some(presented) => {
                let rgba = presented
                    .expand_rgba8()
                    .map_err(SdlHostError::FrameConversion)?;
                upload_rgba_frame(&mut texture, &rgba)?;
                redraw_present(&mut canvas, &texture)?;
            }
            None => {
                // A route that composes no world scene: clear to black until the
                // recovered front-end/overlay compositors are wired for this path.
                canvas.clear();
                if !canvas.present() {
                    return Err(SdlHostError::sdl("frame presentation", sdl3::get_error()));
                }
            }
        }
    }

    Ok(())
}

/// What the interactive loop should do after a batch of host events.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RootLoopDecision {
    /// A shutdown was requested (window close / quit).
    pub quit: bool,
    /// The window is focused and the loop should tick and render.
    pub active: bool,
    /// A focus gain restored lost surfaces; the frame must be re-presented.
    pub restore_surfaces: bool,
}

/// Fold the lifecycle-relevant host events in a batch through the recovered
/// host lifecycle ([`cmr_core::host_lifecycle`]) and summarize what the
/// interactive native loop should do: quit, pause (when unfocused), or
/// re-present after a focus gain restored surfaces. Non-lifecycle events
/// (input, gamepad, redraw) are ignored here and handled by the loop directly.
pub fn advance_root_lifecycle(
    events: &[HostEvent],
    lifecycle: &mut HostLifecycle,
) -> RootLoopDecision {
    let mut restore_surfaces = false;
    for event in events {
        if let Some(lifecycle_event) = event.as_lifecycle() {
            let transition = lifecycle.apply(lifecycle_event);
            if transition
                .effects
                .iter()
                .any(|effect| matches!(effect, WindowEffect::RestoreLostSurfaces))
            {
                restore_surfaces = true;
            }
        }
    }
    RootLoopDecision {
        quit: lifecycle.is_shutting_down(),
        active: lifecycle.is_active(),
        restore_surfaces,
    }
}

fn feed_input_event(event: &Event, mapper: &mut FrontendInputMapper) {
    match event {
        Event::KeyDown {
            keycode: Some(keycode),
            ..
        } => mapper.apply_key(*keycode, true),
        Event::KeyUp {
            keycode: Some(keycode),
            ..
        } => mapper.apply_key(*keycode, false),
        Event::GamepadButtonDown { button, .. } => mapper.apply_button(*button, true),
        Event::GamepadButtonUp { button, .. } => mapper.apply_button(*button, false),
        Event::GamepadAxisMotion { axis, value, .. } => mapper.apply_axis(*axis, *value),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_lifecycle_pauses_on_focus_loss_and_resumes_on_gain() {
        let mut lifecycle = HostLifecycle::new(WindowMode::Windowed);
        lifecycle.apply(PlatformLifecycleEvent::FocusGained);

        // Non-lifecycle events leave the loop active and rendering.
        let decision = advance_root_lifecycle(
            &[HostEvent::RedrawRequested, HostEvent::GamepadAdded(1)],
            &mut lifecycle,
        );
        assert_eq!(
            decision,
            RootLoopDecision {
                quit: false,
                active: true,
                restore_surfaces: false,
            }
        );

        // Focus loss pauses the loop.
        let decision = advance_root_lifecycle(&[HostEvent::FocusLost], &mut lifecycle);
        assert!(!decision.active);
        assert!(!decision.quit);

        // Focus gain resumes and restores the surfaces owed by the loss.
        let decision = advance_root_lifecycle(&[HostEvent::FocusGained], &mut lifecycle);
        assert!(decision.active);
        assert!(decision.restore_surfaces);

        // A close request quits regardless of focus.
        let decision = advance_root_lifecycle(&[HostEvent::QuitRequested], &mut lifecycle);
        assert!(decision.quit);
    }

    #[test]
    fn host_events_bridge_to_backend_neutral_lifecycle_events() {
        assert_eq!(
            HostEvent::QuitRequested.as_lifecycle(),
            Some(PlatformLifecycleEvent::CloseRequested)
        );
        assert_eq!(
            HostEvent::FocusGained.as_lifecycle(),
            Some(PlatformLifecycleEvent::FocusGained)
        );
        assert_eq!(
            HostEvent::FocusLost.as_lifecycle(),
            Some(PlatformLifecycleEvent::FocusLost)
        );
        // Input and redraw events are not lifecycle events.
        for event in [
            HostEvent::EscapePressed,
            HostEvent::GamepadAdded(1),
            HostEvent::RedrawRequested,
            HostEvent::RendererReset,
            HostEvent::Ignored,
        ] {
            assert_eq!(event.as_lifecycle(), None);
        }
    }

    #[test]
    fn focus_events_track_window_focus_without_requesting_exit() {
        let mut state = HostEventState::default();
        assert!(state.focused());
        state.apply(HostEvent::FocusLost);
        assert!(!state.focused());
        assert!(!state.exit_requested());
        state.apply(HostEvent::FocusGained);
        assert!(state.focused());
        assert!(!state.exit_requested());
    }

    #[test]
    fn presentation_scaling_maps_to_sdl_logical_presentation() {
        use sdl3::sys::render::{
            SDL_LOGICAL_PRESENTATION_INTEGER_SCALE, SDL_LOGICAL_PRESENTATION_LETTERBOX,
            SDL_LOGICAL_PRESENTATION_STRETCH,
        };
        assert_eq!(
            logical_presentation(PresentationScaling::Stretch),
            SDL_LOGICAL_PRESENTATION_STRETCH
        );
        assert_eq!(
            logical_presentation(PresentationScaling::Letterbox),
            SDL_LOGICAL_PRESENTATION_LETTERBOX
        );
        assert_eq!(
            logical_presentation(PresentationScaling::IntegerScale),
            SDL_LOGICAL_PRESENTATION_INTEGER_SCALE
        );
        // The backend presents with the project-default scaling.
        assert_eq!(
            logical_presentation(DEFAULT_SCALING),
            SDL_LOGICAL_PRESENTATION_INTEGER_SCALE
        );
    }

    #[test]
    fn quit_and_escape_are_terminal_host_events() {
        for event in [HostEvent::QuitRequested, HostEvent::EscapePressed] {
            let mut state = HostEventState::default();
            state.apply(event);
            assert!(state.exit_requested());
        }
    }

    #[test]
    fn gamepad_hotplug_is_idempotent_and_preserves_other_devices() {
        let mut state = HostEventState::with_connected_gamepads([7, 9]);
        state.apply(HostEvent::GamepadAdded(9));
        state.apply(HostEvent::GamepadAdded(11));
        state.apply(HostEvent::GamepadRemoved(7));
        state.apply(HostEvent::GamepadRemoved(404));

        assert_eq!(state.connected_gamepads(), &BTreeSet::from([9, 11]));
        assert!(!state.exit_requested());
    }

    #[test]
    fn unrelated_events_do_not_change_host_state() {
        let mut state = HostEventState::with_connected_gamepads([3]);
        let before = state.clone();
        state.apply(HostEvent::Ignored);
        assert_eq!(state, before);
    }

    #[test]
    fn packed_rows_respect_an_sdl_owned_destination_pitch() {
        let source = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut destination = [0xaa; 12];

        copy_packed_rows(&source, 4, &mut destination, 6, 2).unwrap();

        assert_eq!(
            destination,
            [1, 2, 3, 4, 0xaa, 0xaa, 5, 6, 7, 8, 0xaa, 0xaa]
        );
    }

    #[test]
    fn packed_rows_reject_short_buffers_and_narrow_destination_pitch() {
        let mut destination = [0; 8];
        assert!(matches!(
            copy_packed_rows(&[1; 7], 4, &mut destination, 4, 2),
            Err(SdlHostError::InvalidUploadLayout { .. })
        ));
        assert!(matches!(
            copy_packed_rows(&[1; 8], 4, &mut destination, 3, 2),
            Err(SdlHostError::InvalidUploadLayout { .. })
        ));
    }
}
