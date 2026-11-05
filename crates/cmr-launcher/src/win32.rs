//! The WinMM joystick reads (the API the game itself uses) for F7 and
//! `--controllers`: the Win32 calls the SDK (`rea-launcher-win32`) doesn't
//! make. Registry, dialogs, XInput, GPU choice, Wine detection and starting
//! the game are the SDK's. The launcher never writes into the running game:
//! the fixes and the hook library are in the runtime copy's executable
//! (`cmr_launcher::patched_exe`).

use windows_sys::Win32::Media::Multimedia::{
    JOYCAPSW, JOYERR_NOERROR, JOYINFOEX, joyGetDevCapsW, joyGetPosEx,
};

/// One `joyGetPosEx` read with the given `JOY_RETURN*` flags; the error code
/// on failure.
fn joy_pos(id: u32, flags: u32) -> Result<JOYINFOEX, u32> {
    let mut info = JOYINFOEX {
        dwSize: size_of::<JOYINFOEX>() as u32,
        dwFlags: flags,
        ..Default::default()
    };
    match unsafe { joyGetPosEx(id, &mut info) } {
        JOYERR_NOERROR => Ok(info),
        err => Err(err),
    }
}

/// Lists the joysticks WinMM reports (the API the game reads them with, ids
/// 0..15 in the game's order), then watches them for `seconds` and logs
/// every axis move past a quarter of its range and every button change, so a
/// user can see which control reaches the game as which axis or button.
pub fn controllers(seconds: u32, log: &mut dyn FnMut(&str)) {
    const AXES: [&str; 6] = ["X", "Y", "Z", "R", "U", "V"];
    let read = |id: u32| {
        // JOY_RETURNALL: X Y Z R U V, hat, buttons
        joy_pos(id, 0xff).ok().map(|info| {
            let axes = [
                info.dwXpos,
                info.dwYpos,
                info.dwZpos,
                info.dwRpos,
                info.dwUpos,
                info.dwVpos,
            ];
            (axes, info.dwButtons, info.dwPOV)
        })
    };
    let mut live = Vec::new();
    for id in 0..16u32 {
        let mut caps = JOYCAPSW::default();
        if unsafe { joyGetDevCapsW(id as usize, &mut caps, size_of::<JOYCAPSW>() as u32) }
            != JOYERR_NOERROR
        {
            continue;
        }
        let (pname, axes, buttons, flags) =
            (caps.szPname, caps.wNumAxes, caps.wNumButtons, caps.wCaps);
        let name = String::from_utf16_lossy(&pname);
        let name = name.trim_end_matches('\0');
        let state = read(id);
        log(&format!(
            "joystick {} (game: JOYSTICK {}): \"{name}\", {} axes, {} buttons, caps 0x{:x}{}",
            id,
            id + 1,
            axes,
            buttons,
            flags,
            if state.is_some() {
                ""
            } else {
                ", not answering"
            }
        ));
        // The game's own analog read: X, Y, buttons, JOY_RETURNCENTERED.
        let mut game = JOYINFOEX {
            dwSize: size_of::<JOYINFOEX>() as u32,
            dwFlags: 0x483,
            ..Default::default()
        };
        let r = unsafe { joyGetPosEx(id, &mut game) };
        let (gx, gy, gb) = (game.dwXpos, game.dwYpos, game.dwButtons);
        log(&format!(
            "  as the game reads it (0x483): result {r}, X {gx}, Y {gy}, buttons 0x{gb:x}"
        ));
        if let Some(s) = state {
            log(&format!("  at rest: X Y Z R U V = {:?}, hat {}", s.0, s.2));
            live.push((id, s));
        }
    }
    if live.is_empty() {
        log("no joysticks found");
        return;
    }
    log(&format!(
        "watching for {seconds} s: move every stick and trigger, press every button"
    ));
    let steps = seconds * 20;
    for _ in 0..steps {
        std::thread::sleep(std::time::Duration::from_millis(50));
        for (id, last) in live.iter_mut() {
            let Some(now) = read(*id) else { continue };
            for (i, name) in AXES.iter().enumerate() {
                let (a, b) = (last.0[i] as i64, now.0[i] as i64);
                if (a - b).abs() > 0x4000 {
                    log(&format!("joystick {}: axis {name} {a} -> {b}", *id + 1));
                    last.0[i] = now.0[i];
                }
            }
            let changed = last.1 ^ now.1;
            for bit in 0..32 {
                if changed & (1 << bit) != 0 {
                    let state = if now.1 & (1 << bit) != 0 {
                        "down"
                    } else {
                        "up"
                    };
                    log(&format!("joystick {}: button {} {state}", *id + 1, bit + 1));
                }
            }
            last.1 = now.1;
            if now.2 != last.2 {
                log(&format!(
                    "joystick {}: hat {} -> {}",
                    *id + 1,
                    last.2,
                    now.2
                ));
                last.2 = now.2;
            }
        }
    }
    log("done watching");
}

/// For F7: per WinMM id, which extra axes (Z, R) exist and rest near centre
/// now. Pedals and sliders rest at one end, so they are left out.
pub fn centred_axes() -> [u32; 16] {
    use cmr_launcher::fixes::{AXIS_R, AXIS_Z};
    const JOYCAPS_HASZ: u32 = 1;
    const JOYCAPS_HASR: u32 = 2;
    let near_centre = |v: u32| (v as i64 - 0x8000).abs() < 0x2000;
    let mut table = [0u32; 16];
    for (id, slot) in table.iter_mut().enumerate() {
        let mut caps = JOYCAPSW::default();
        if unsafe { joyGetDevCapsW(id, &mut caps, size_of::<JOYCAPSW>() as u32) } != JOYERR_NOERROR
        {
            continue;
        }
        // X Y Z R and buttons
        let Ok(info) = joy_pos(id as u32, 0x8f) else {
            continue;
        };
        let (has, z, r) = (caps.wCaps, info.dwZpos, info.dwRpos);
        if has & JOYCAPS_HASZ != 0 && near_centre(z) {
            *slot |= AXIS_Z;
        }
        if has & JOYCAPS_HASR != 0 && near_centre(r) {
            *slot |= AXIS_R;
        }
    }
    table
}
