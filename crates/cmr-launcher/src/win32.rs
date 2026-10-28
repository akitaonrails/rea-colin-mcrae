//! All Win32 calls. Kept small: process start + memory writes, registry,
//! message boxes.

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, HANDLE,
};
use windows_sys::Win32::System::Diagnostics::Debug::{
    FlushInstructionCache, ReadProcessMemory, WriteProcessMemory,
};
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, VirtualAllocEx,
    VirtualProtectEx,
};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_QUERY_VALUE, KEY_SET_VALUE, KEY_WOW64_32KEY,
    REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW, RegQueryValueExW,
    RegSetValueExW,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateProcessW, GetExitCodeProcess, INFINITE, PROCESS_INFORMATION,
    ResumeThread, STARTUPINFOW, TerminateProcess, WaitForSingleObject,
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

fn write(
    process: HANDLE,
    address: u32,
    bytes: &[u8],
    final_protect: Option<u32>,
) -> Result<(), String> {
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
        VirtualProtectEx(
            process,
            at,
            bytes.len(),
            final_protect.unwrap_or(old),
            &mut ignored,
        );
        if ok == 0 || done != bytes.len() {
            return Err(format!(
                "cannot write game memory at 0x{address:08x}: {}",
                last_error()
            ));
        }
    }
    Ok(())
}

/// Starts `exe` suspended, applies the fixes, resumes it and waits for it
/// to exit. Allocates `cave_size` bytes for the fixes' own code (none when
/// 0); `prepare` turns that address into checked writes.
pub fn run_patched(
    exe: &Path,
    workdir: &Path,
    cave_size: u32,
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
        let protect = (!w.write.in_image).then_some(PAGE_EXECUTE_READ);
        write(process, w.write.address, &w.write.bytes, protect)?;
        log(&format!(
            "{} applied at 0x{:08x} ({} bytes)",
            w.write.fix,
            w.write.address,
            w.write.bytes.len()
        ));
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
