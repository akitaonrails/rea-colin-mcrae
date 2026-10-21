//! SDL3 host adapter for completed compatibility frames.
//!
//! SDL owns the native window, event queue, and opened gamepad handles. It
//! does not participate in recovered rasterization: callers supply one
//! completed indexed software frame, which is expanded to opaque RGBA8 before
//! upload to an SDL streaming texture.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cmr_video::{SoftwarePresentedFrame, SoftwareRgba8Error, SoftwareRgba8Frame};
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::pixels::PixelFormat;
use sdl3::render::{Canvas, ScaleMode, Texture};
use sdl3::video::Window;

/// Platform-neutral events relevant to the first native host loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostEvent {
    QuitRequested,
    EscapePressed,
    GamepadAdded(u32),
    GamepadRemoved(u32),
    RedrawRequested,
    RendererReset,
    Ignored,
}

/// Testable lifecycle state mirrored by the SDL event adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostEventState {
    exit_requested: bool,
    connected_gamepads: BTreeSet<u32>,
}

impl HostEventState {
    #[must_use]
    pub fn with_connected_gamepads(ids: impl IntoIterator<Item = u32>) -> Self {
        Self {
            exit_requested: false,
            connected_gamepads: ids.into_iter().collect(),
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
            HostEvent::RedrawRequested | HostEvent::RendererReset | HostEvent::Ignored => {}
        }
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
            sdl3::sys::render::SDL_LOGICAL_PRESENTATION_INTEGER_SCALE,
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

#[cfg(test)]
mod tests {
    use super::*;

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
