use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr;
use std::sync::Mutex;
use std::time::Instant;

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    FreeSid, GetTokenInformation, PSID, SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES, TOKEN_QUERY,
    TokenIsAppContainer,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW,
    DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
    GetExitCodeProcess, InitializeProcThreadAttributeList, OpenProcessToken,
    PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
    PROCESS_INFORMATION, ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    UpdateProcThreadAttribute,
};

use crate::Error;

pub struct Guard {
    job: usize,
    process: usize,
    until: Mutex<Option<Instant>>,
}

impl Guard {
    pub fn deadline(&self, until: Option<Instant>) {
        *self.until.lock().unwrap() = until;
    }

    pub fn expired(&self) -> bool {
        self.until
            .lock()
            .unwrap()
            .is_some_and(|at| Instant::now() >= at)
    }

    pub fn kill(&self) {
        unsafe { TerminateJobObject(self.job as HANDLE, 1) };
    }

    pub fn is_alive(&self) -> bool {
        let mut code = 0;
        unsafe { GetExitCodeProcess(self.process as HANDLE, &mut code) != 0 && code == 259 }
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        self.kill();
        unsafe {
            CloseHandle(self.process as HANDLE);
            CloseHandle(self.job as HANDLE);
        }
    }
}

struct Sid(PSID);
impl Drop for Sid {
    fn drop(&mut self) {
        unsafe { FreeSid(self.0) };
    }
}

struct Attributes {
    memory: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn as_ptr(&mut self) -> *mut c_void {
        self.memory.as_mut_ptr().cast()
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
        }
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn win_error(context: &str) -> Error {
    Error::new(format!("{context}: {}", std::io::Error::last_os_error()))
}

pub fn spawn(exe: &std::path::Path, id: &str) -> Result<super::Pipes, Error> {
    let exe_w = wide(exe.as_os_str());
    let mut command = wide(std::ffi::OsStr::new(&format!(
        "\"{}\" --nyapp-worker",
        exe.display()
    )));
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| Error::new("LOCALAPPDATA is required to launch an AppContainer"))?;
    let mut environment = std::ffi::OsString::from("LOCALAPPDATA=");
    environment.push(local_app_data);
    environment.push("\0SystemRoot=");
    environment.push(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    let mut environment = wide(&environment);
    environment.push(0);
    // AppContainer 名の 64 文字制限と、ニャプリ ID の 128 文字上限を両立する。
    let digest = Sha256::digest(id.as_bytes());
    let profile_name = format!(
        "nyaw.nyapp.{}",
        digest[..24]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let profile = wide(std::ffi::OsStr::new(&profile_name));
    let mut sid: PSID = ptr::null_mut();
    let hr = unsafe {
        CreateAppContainerProfile(
            profile.as_ptr(),
            profile.as_ptr(),
            profile.as_ptr(),
            ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr as u32 == 0x8007_00b7 {
        let hr = unsafe { DeriveAppContainerSidFromAppContainerName(profile.as_ptr(), &mut sid) };
        if hr < 0 {
            return Err(Error::new(format!(
                "cannot derive AppContainer SID: 0x{hr:08x}"
            )));
        }
    } else if hr < 0 {
        return Err(Error::new(format!(
            "cannot create AppContainer: 0x{hr:08x}"
        )));
    }
    let sid = Sid(sid);
    let mut security = SECURITY_CAPABILITIES {
        AppContainerSid: sid.0,
        Capabilities: ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    let pipe_security = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let (mut child_input, mut parent_input, mut parent_output, mut child_output) = (
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    );
    if unsafe { CreatePipe(&mut child_input, &mut parent_input, &pipe_security, 0) } == 0 {
        return Err(win_error("CreatePipe stdin"));
    }
    let child_input = unsafe { OwnedHandle::from_raw_handle(child_input) };
    let parent_input = unsafe { OwnedHandle::from_raw_handle(parent_input) };
    if unsafe { CreatePipe(&mut parent_output, &mut child_output, &pipe_security, 0) } == 0 {
        return Err(win_error("CreatePipe stdout"));
    }
    let parent_output = unsafe { OwnedHandle::from_raw_handle(parent_output) };
    let child_output = unsafe { OwnedHandle::from_raw_handle(child_output) };
    if unsafe {
        SetHandleInformation(parent_input.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0
            || SetHandleInformation(parent_output.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0
    } {
        return Err(win_error("pipe inheritance"));
    }

    let mut size = 0usize;
    unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), 2, 0, &mut size) };
    let mut attrs = Attributes {
        memory: vec![0usize; size.div_ceil(std::mem::size_of::<usize>())],
        initialized: false,
    };
    if unsafe { InitializeProcThreadAttributeList(attrs.as_ptr(), 2, 0, &mut size) } == 0 {
        return Err(win_error("attribute list"));
    }
    attrs.initialized = true;
    let handles = [child_input.as_raw_handle(), child_output.as_raw_handle()];
    if unsafe {
        UpdateProcThreadAttribute(
            attrs.as_ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (&mut security as *mut SECURITY_CAPABILITIES).cast(),
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            ptr::null_mut(),
            ptr::null(),
        ) == 0
            || UpdateProcThreadAttribute(
                attrs.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(&handles),
                ptr::null_mut(),
                ptr::null(),
            ) == 0
    } {
        return Err(win_error("AppContainer attributes"));
    }

    let job = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if job.is_null() {
        return Err(win_error("CreateJobObject"));
    }
    let job = unsafe { OwnedHandle::from_raw_handle(job) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.ProcessMemoryLimit = 512 << 20;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(win_error("SetInformationJobObject"));
    }

    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = child_input.as_raw_handle();
    startup.StartupInfo.hStdOutput = child_output.as_raw_handle();
    startup.lpAttributeList = attrs.as_ptr();
    let mut process = PROCESS_INFORMATION::default();
    if unsafe {
        CreateProcessW(
            exe_w.as_ptr(),
            command.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            EXTENDED_STARTUPINFO_PRESENT
                | CREATE_SUSPENDED
                | CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT,
            environment.as_ptr().cast(),
            ptr::null(),
            (&startup as *const STARTUPINFOEXW).cast(),
            &mut process,
        )
    } == 0
    {
        return Err(win_error("CreateProcess AppContainer"));
    }
    let process_handle = unsafe { OwnedHandle::from_raw_handle(process.hProcess) };
    let thread_handle = unsafe { OwnedHandle::from_raw_handle(process.hThread) };
    if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process_handle.as_raw_handle()) } == 0
    {
        unsafe { TerminateProcess(process_handle.as_raw_handle(), 1) };
        return Err(win_error("AssignProcessToJobObject"));
    }
    if unsafe { ResumeThread(thread_handle.as_raw_handle()) } == u32::MAX {
        unsafe { TerminateJobObject(job.as_raw_handle(), 1) };
        return Err(win_error("ResumeThread"));
    }
    let guard = Guard {
        job: job.as_raw_handle() as usize,
        process: process_handle.as_raw_handle() as usize,
        until: Mutex::new(None),
    };
    std::mem::forget(job);
    std::mem::forget(process_handle);
    Ok((
        Box::new(std::fs::File::from(parent_input)),
        Box::new(std::fs::File::from(parent_output)),
        guard,
    ))
}

/// 誤った引数や直接起動で通常のトークンのまま Wasm を実行しない。
pub fn confine() -> Result<(), Error> {
    let mut raw = ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(win_error("OpenProcessToken"));
    }
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut is_app_container = 0u32;
    let mut size = 0;
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenIsAppContainer,
            (&mut is_app_container as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
            &mut size,
        )
    } == 0
    {
        return Err(win_error("GetTokenInformation"));
    }
    if is_app_container != 1 {
        return Err(Error::new("the nyapp worker is not in an AppContainer"));
    }
    Ok(())
}
