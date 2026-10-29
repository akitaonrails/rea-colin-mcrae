//! All Win32 calls. Kept small: process start + memory writes, registry,
//! message boxes.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, FreeLibrary, HANDLE,
};
use windows_sys::Win32::Media::Multimedia::{
    JOYCAPSW, JOYERR_NOERROR, JOYINFOEX, joyGetDevCapsW, joyGetPosEx,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, ReadProcessMemory, WriteProcessMemory,
};
use windows_sys::Win32::System::LibraryLoader::{
    DONT_RESOLVE_DLL_REFERENCES, GetModuleHandleW, GetProcAddress, LoadLibraryExW,
};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAllocEx,
    VirtualFreeEx, VirtualProtectEx,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY,
    REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegQueryValueExW,
    RegSetValueExW,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessW, CreateRemoteThread, GetExitCodeProcess, GetExitCodeThread,
    INFINITE, PROCESS_INFORMATION, ResumeThread, STARTUPINFOW, TerminateProcess,
    WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MessageBoxW,
};

use cmr_launcher::CheckedWrite;
use cmr_launcher::registry::{KEY, VIRTUAL_STORE_KEY, Value, When};

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

pub fn message(title: &str, text: &str, error: bool) {
    let icon = if error {
        MB_ICONERROR
    } else {
        MB_ICONINFORMATION
    };
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide(title).as_ptr(),
            MB_OK | icon,
        );
    }
}

/// True when running under Wine or Proton.
pub fn is_wine() -> bool {
    unsafe {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}

fn create_key(root: HKEY, path: &str, wow32: bool) -> Result<Key, u32> {
    let mut key: HKEY = null_mut();
    let access = KEY_QUERY_VALUE | KEY_SET_VALUE | if wow32 { KEY_WOW64_32KEY } else { 0 };
    let rc = unsafe {
        RegCreateKeyExW(
            root,
            wide(path).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            access,
            null(),
            &mut key,
            null_mut(),
        )
    };
    if rc == ERROR_SUCCESS {
        Ok(Key(key))
    } else {
        Err(rc)
    }
}

/// Writes the game's registry values. Returns where they went.
pub fn write_registry(values: &[Value]) -> Result<&'static str, String> {
    let (key, place) = match create_key(HKEY_LOCAL_MACHINE, KEY, true) {
        Ok(k) => (k, "HKLM"),
        Err(ERROR_ACCESS_DENIED) => (
            create_key(HKEY_CURRENT_USER, VIRTUAL_STORE_KEY, false)
                .map_err(|rc| format!("cannot open the registry key (error {rc})"))?,
            "HKCU VirtualStore",
        ),
        Err(rc) => return Err(format!("cannot open the registry key (error {rc})")),
    };
    for v in values {
        let name = wide(v.name);
        if v.when == When::IfMissing {
            let rc = unsafe {
                RegQueryValueExW(
                    key.0,
                    name.as_ptr(),
                    null(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            if rc != ERROR_FILE_NOT_FOUND {
                continue;
            }
        }
        let data = wide(&v.data);
        let rc = unsafe {
            RegSetValueExW(
                key.0,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!(
                "cannot write registry value {} (error {rc})",
                v.name
            ));
        }
    }
    Ok(place)
}

struct Child {
    info: PROCESS_INFORMATION,
    resumed: bool,
}

impl Drop for Child {
    fn drop(&mut self) {
        unsafe {
            if !self.resumed {
                TerminateProcess(self.info.hProcess, 1);
            }
            CloseHandle(self.info.hThread);
            CloseHandle(self.info.hProcess);
        }
    }
}

fn read(process: HANDLE, address: u32, len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    let mut got = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            process,
            address as usize as *const c_void,
            buf.as_mut_ptr().cast(),
            len,
            &mut got,
        )
    };
    if ok == 0 || got != len {
        return Err(format!(
            "cannot read game memory at 0x{address:08x}: {}",
            last_error()
        ));
    }
    Ok(buf)
}

/// Writes `bytes` and restores the page protection that was there: the
/// game's code pages, or our cave, which stays read-write-execute because
/// fixes keep state in it (F8).
fn write(process: HANDLE, address: u32, bytes: &[u8]) -> Result<(), String> {
    let at = address as usize as *const c_void;
    let mut old = 0u32;
    unsafe {
        if VirtualProtectEx(process, at, bytes.len(), PAGE_EXECUTE_READWRITE, &mut old) == 0 {
            return Err(format!(
                "cannot unprotect 0x{address:08x}: {}",
                last_error()
            ));
        }
        let mut done = 0usize;
        let ok = WriteProcessMemory(process, at, bytes.as_ptr().cast(), bytes.len(), &mut done);
        let mut ignored = 0u32;
        VirtualProtectEx(process, at, bytes.len(), old, &mut ignored);
        if ok == 0 || done != bytes.len() {
            return Err(format!(
                "cannot write game memory at 0x{address:08x}: {}",
                last_error()
            ));
        }
    }
    Ok(())
}

/// A DLL to load into the game before it runs, and where in the fix memory
/// its exports go (see `fixes::F13_TABLE`).
pub struct HookLibrary<'a> {
    pub path: &'a Path,
    pub table: u32,
    pub exports: &'a [&'a str],
}

/// Each export's offset from the DLL's base, read by mapping the DLL here
/// without running it (both processes are 32-bit).
fn export_offsets(path: &Path, names: &[&str]) -> Result<Vec<u32>, String> {
    let wpath = wide(path);
    let module = unsafe { LoadLibraryExW(wpath.as_ptr(), null_mut(), DONT_RESOLVE_DLL_REFERENCES) };
    if module.is_null() {
        return Err(format!("cannot open {}: {}", path.display(), last_error()));
    }
    let offsets = names
        .iter()
        .map(|name| {
            let cname = format!("{name}\0");
            match unsafe { GetProcAddress(module, cname.as_ptr()) } {
                Some(f) => Ok((f as usize - module as usize) as u32),
                None => Err(format!("{} has no export {name}", path.display())),
            }
        })
        .collect();
    unsafe { FreeLibrary(module) };
    offsets
}

/// Loads the hook library into the (suspended) game with a remote
/// `LoadLibraryW` and writes its exports' addresses into the fix memory.
/// Returns the library's base address in the game.
fn load_hooks(process: HANDLE, hooks: &HookLibrary, cave: u32) -> Result<u32, String> {
    let offsets = export_offsets(hooks.path, hooks.exports)?;
    let wpath = wide(hooks.path);
    let size = wpath.len() * 2;
    let remote = unsafe {
        VirtualAllocEx(
            process,
            null(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote.is_null() {
        return Err(format!("cannot allocate in the game: {}", last_error()));
    }
    let result = (|| {
        let mut done = 0usize;
        if unsafe { WriteProcessMemory(process, remote, wpath.as_ptr().cast(), size, &mut done) }
            == 0
        {
            return Err(format!("cannot write the library path: {}", last_error()));
        }
        let kernel32 = wide("kernel32.dll");
        let load = unsafe {
            GetProcAddress(
                GetModuleHandleW(kernel32.as_ptr()),
                c"LoadLibraryW".as_ptr().cast(),
            )
        }
        .ok_or("no LoadLibraryW")?;
        let thread = unsafe {
            CreateRemoteThread(
                process,
                null(),
                0,
                // LoadLibraryW(path) as the thread routine: one pointer in,
                // a 32-bit module handle out (the thread's exit code).
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void) -> u32,
                >(load)),
                remote,
                0,
                null_mut(),
            )
        };
        if thread.is_null() {
            return Err(format!("cannot start the loader thread: {}", last_error()));
        }
        let mut base = 0u32;
        unsafe {
            WaitForSingleObject(thread, 20_000);
            GetExitCodeThread(thread, &mut base);
            CloseHandle(thread);
        }
        if base == 0 {
            return Err("the game could not load the library".into());
        }
        let table: Vec<u8> = offsets
            .iter()
            .flat_map(|o| base.wrapping_add(*o).to_le_bytes())
            .collect();
        write(process, cave.wrapping_add(hooks.table), &table)?;
        Ok(base)
    })();
    unsafe { VirtualFreeEx(process, remote, 0, MEM_RELEASE) };
    result
}

/// Starts `exe` suspended, applies the fixes, resumes it and waits for it
/// to exit. Allocates `cave_size` bytes for the fixes' own code (none when
/// 0); `prepare` turns that address into checked writes.
pub fn run_patched(
    exe: &Path,
    workdir: &Path,
    cave_size: u32,
    hooks: Option<HookLibrary>,
    prepare: impl FnOnce(u32) -> Result<Vec<CheckedWrite>, String>,
    log: &mut dyn FnMut(&str),
) -> Result<u32, String> {
    let app = wide(exe);
    let mut cmdline = wide(format!("\"{}\"", exe.display()));
    let dir = wide(workdir);
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = size_of::<STARTUPINFOW>() as u32;
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            app.as_ptr(),
            cmdline.as_mut_ptr(),
            null(),
            null(),
            0,
            CREATE_SUSPENDED,
            null(),
            dir.as_ptr(),
            &si,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(format!("cannot start {}: {}", exe.display(), last_error()));
    }
    let mut child = Child {
        info,
        resumed: false,
    };
    let process = child.info.hProcess;

    let cave = if cave_size == 0 {
        0
    } else {
        let p = unsafe {
            VirtualAllocEx(
                process,
                null(),
                cave_size as usize,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_EXECUTE_READWRITE,
            )
        };
        if p.is_null() {
            return Err(format!("cannot allocate fix memory: {}", last_error()));
        }
        p as usize as u32
    };
    let writes = prepare(cave)?;

    // Check every site before changing anything.
    for w in writes.iter().filter(|w| w.write.in_image) {
        let now = read(process, w.write.address, w.original.len())?;
        if now != w.original {
            return Err(format!(
                "{}: game memory at 0x{:08x} does not match the executable; not starting",
                w.write.fix, w.write.address
            ));
        }
    }
    for w in &writes {
        write(process, w.write.address, &w.write.bytes)?;
        log(&format!(
            "{} applied at 0x{:08x} ({} bytes)",
            w.write.fix,
            w.write.address,
            w.write.bytes.len()
        ));
    }
    if let Some(h) = &hooks {
        match load_hooks(process, h, cave) {
            Ok(base) => log(&format!("{} loaded at 0x{base:08x}", h.path.display())),
            // The fixes' stubs see an empty table and do nothing.
            Err(e) => log(&format!(
                "warning: hook library not loaded ({e}); its fixes stay off"
            )),
        }
    }
    unsafe {
        FlushInstructionCache(process, null(), 0);
        if ResumeThread(child.info.hThread) == u32::MAX {
            return Err(format!("cannot resume the game: {}", last_error()));
        }
    }
    child.resumed = true;
    log("game started");
    let mut code = 0u32;
    unsafe {
        WaitForSingleObject(process, INFINITE);
        GetExitCodeProcess(process, &mut code);
    }
    Ok(code)
}

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
