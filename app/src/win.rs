//! Chamadas Win32 que o controlador precisa e que o std não expõe.
use std::fs::File;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Storage::FileSystem::{
    LockFileEx, UnlockFileEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY,
};
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, TerminateProcess, WaitForSingleObject,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

const STILL_ACTIVE: u32 = 259;

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open(pid: u32) -> Option<Handle> {
    let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE;
    let h = unsafe { OpenProcess(access, 0, pid) };
    (!h.is_null()).then_some(Handle(h))
}

/// Início do processo em segundos Unix, igual ao `psutil.Process.create_time()`.
pub fn creation_time(pid: u32) -> Option<f64> {
    let h = open(pid)?;
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    if unsafe { GetProcessTimes(h.0, &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let ticks = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    Some((ticks as f64 - 116_444_736_000_000_000.0) / 10_000_000.0)
}

pub fn is_running(pid: u32) -> bool {
    let Some(h) = open(pid) else { return false };
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(h.0, &mut code) != 0 && code == STILL_ACTIVE }
}

pub fn terminate(pid: u32) {
    if let Some(h) = open(pid) {
        unsafe { TerminateProcess(h.0, 1) };
    }
}

/// Espera o processo terminar; `true` se ele não está mais em execução.
pub fn wait(pid: u32, timeout: Duration) -> bool {
    let Some(h) = open(pid) else { return true };
    unsafe { WaitForSingleObject(h.0, timeout.as_millis() as u32) == WAIT_OBJECT_0 }
}

/// Impede que serviços iniciados herdem o console ou o pipe de quem chamou o
/// controlador; do contrário um `ordis.exe iniciar | ...` nunca terminaria.
pub fn nao_herdar_console() {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
    for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let h = GetStdHandle(id);
            if !h.is_null() && h as isize != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

/// Trava o primeiro byte do arquivo, compatível com `msvcrt.locking(..., LK_NBLCK, 1)`.
pub struct ByteLock<'a>(&'a File);

impl<'a> ByteLock<'a> {
    pub fn try_lock(file: &'a File) -> Option<Self> {
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        let flags = LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY;
        let ok = unsafe { LockFileEx(file.as_raw_handle() as HANDLE, flags, 0, 1, 0, &mut ov) };
        (ok != 0).then_some(ByteLock(file))
    }
}

impl Drop for ByteLock<'_> {
    fn drop(&mut self) {
        let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
        unsafe { UnlockFileEx(self.0.as_raw_handle() as HANDLE, 0, 1, 0, &mut ov) };
    }
}
