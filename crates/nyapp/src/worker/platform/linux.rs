use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::Instant;

use crate::Error;

pub struct Guard {
    child: Mutex<Child>,
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
        let _ = self.child.lock().unwrap().kill();
    }

    pub fn is_alive(&self) -> bool {
        matches!(self.child.lock().unwrap().try_wait(), Ok(None))
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn spawn(exe: &std::path::Path, _: &str) -> Result<super::Pipes, Error> {
    let mut child = Command::new(exe)
        .arg("--nyapp-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_clear()
        .env("NYAW_WORKER_PARENT_PID", std::process::id().to_string())
        .spawn()?;
    let input = child
        .stdin
        .take()
        .ok_or_else(|| Error::new("no worker stdin"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| Error::new("no worker stdout"))?;
    Ok((
        Box::new(input),
        Box::new(output),
        Guard {
            child: Mutex::new(child),
            until: Mutex::new(None),
        },
    ))
}

/// ワーカーはパッケージのバイト列を受け取る前に、ファイル・ネットワーク・プロセス操作を閉じる。
pub fn confine() -> Result<(), Error> {
    let parent: u32 = std::env::var("NYAW_WORKER_PARENT_PID")
        .ok()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| Error::new("missing nyapp parent PID"))?;
    unsafe {
        if libc::geteuid() == 0 {
            if libc::setgroups(0, std::ptr::null()) != 0
                || libc::setresgid(65534, 65534, 65534) != 0
                || libc::setresuid(65534, 65534, 65534) != 0
            {
                return Err(Error::new(format!(
                    "cannot drop root: {}",
                    std::io::Error::last_os_error()
                )));
            }
        }
        if libc::prctl(libc::PR_SET_DUMPABLE, 0) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0
            || libc::getppid() as u32 != parent
        {
            return Err(Error::new("nyapp parent is gone"));
        }
        limit(libc::RLIMIT_AS, 512 << 20)?;
        limit(libc::RLIMIT_NOFILE, 4)?;
        limit(libc::RLIMIT_FSIZE, 0)?;
        limit(libc::RLIMIT_CORE, 0)?;
        limit(libc::RLIMIT_CPU, 300)?;
        if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        landlock()?;
        seccomp()?;
    }
    Ok(())
}

unsafe fn limit(resource: libc::__rlimit_resource_t, max: libc::rlim_t) -> Result<(), Error> {
    let value = libc::rlimit {
        rlim_cur: max,
        rlim_max: max,
    };
    if unsafe { libc::setrlimit(resource, &value) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[repr(C)]
struct LandlockRuleset {
    handled_access_fs: u64,
}

unsafe fn landlock() -> Result<(), Error> {
    const VERSION: i32 = 1;
    let abi = unsafe { libc::syscall(libc::SYS_landlock_create_ruleset, 0, 0, VERSION) };
    if abi < 3 {
        return Err(Error::new("Landlock ABI 3 or newer is required"));
    }
    // ABI 3 までのファイル操作を全て制限。許可ルールは作らない。
    let rules = LandlockRuleset {
        handled_access_fs: (1 << 15) - 1,
    };
    let fd = unsafe {
        libc::syscall(
            libc::SYS_landlock_create_ruleset,
            &rules,
            std::mem::size_of::<LandlockRuleset>(),
            0,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let result = unsafe { libc::syscall(libc::SYS_landlock_restrict_self, fd, 0) };
    unsafe { libc::close(fd as i32) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

unsafe fn seccomp() -> Result<(), Error> {
    const ALLOW: u32 = 0x7fff_0000;
    const ERRNO: u32 = 0x0005_0000;
    const KILL: u32 = 0x8000_0000;
    const ARCH_X86_64: u32 = 0xc000_003e;
    #[cfg(target_arch = "x86_64")]
    const ARCH: u32 = ARCH_X86_64;
    #[cfg(target_arch = "aarch64")]
    const ARCH: u32 = 0xc000_00b7;
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    compile_error!("nyapp seccomp requires an architecture-specific audit value");

    let mut filter = vec![
        stmt(0x20, 4), // seccomp_data.arch
        jump(0x15, ARCH, 1, 0),
        stmt(0x06, KILL),
        stmt(0x20, 0), // seccomp_data.nr
    ];
    for nr in [
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_readv,
        libc::SYS_writev,
        libc::SYS_close,
        libc::SYS_futex,
        libc::SYS_clock_gettime,
        libc::SYS_getrandom,
        libc::SYS_mmap,
        libc::SYS_munmap,
        libc::SYS_mprotect,
        libc::SYS_brk,
        libc::SYS_mremap,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_sigaltstack,
        libc::SYS_rt_sigreturn,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_sched_yield,
        libc::SYS_exit,
        libc::SYS_exit_group,
    ] {
        filter.push(jump(0x15, nr as u32, 0, 1));
        filter.push(stmt(0x06, ALLOW));
    }
    filter.push(stmt(0x06, ERRNO | libc::EPERM as u32));
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_mut_ptr(),
    };
    if unsafe { libc::prctl(libc::PR_SET_SECCOMP, libc::SECCOMP_MODE_FILTER, &program) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}
