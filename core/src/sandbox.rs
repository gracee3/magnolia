#[cfg(target_os = "linux")]
use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
#[cfg(target_os = "linux")]
use std::collections::BTreeMap;

#[cfg(target_os = "linux")]
pub fn create_plugin_sandbox() -> anyhow::Result<BpfProgram> {
    // Define allowed syscalls
    // This is a strict whitelist. Anything not listed will cause EPERM.
    let allowed_syscalls = vec![
        libc::SYS_read,
        libc::SYS_write,
        libc::SYS_open,
        libc::SYS_openat,
        libc::SYS_close,
        libc::SYS_stat,
        libc::SYS_fstat,
        libc::SYS_lstat,
        libc::SYS_lseek,
        libc::SYS_mmap,
        libc::SYS_mprotect,
        libc::SYS_munmap,
        libc::SYS_brk,
        libc::SYS_rt_sigaction,
        libc::SYS_rt_sigprocmask,
        libc::SYS_ioctl,
        libc::SYS_poll,
        libc::SYS_select,
        libc::SYS_nanosleep,
        libc::SYS_getpid,
        libc::SYS_gettid,
        libc::SYS_gettimeofday,
        libc::SYS_clock_gettime,
        libc::SYS_futex,
        libc::SYS_clone, // Threads
        libc::SYS_set_robust_list,
        libc::SYS_exit,
        libc::SYS_exit_group,
        libc::SYS_sched_yield,
        libc::SYS_fcntl,
        libc::SYS_readlink,
        libc::SYS_getcwd,
    ];

    let mut rules = BTreeMap::new();
    for syscall in allowed_syscalls {
        rules.insert(syscall as i64, vec![]);
    }

    // Create filter
    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Errno(libc::EPERM as u32), // Default action: deny with EPERM
        SeccompAction::Allow,                     // Mismatch action (should match rules)
        std::env::consts::ARCH.try_into()?,
    )?;

    Ok(filter.try_into()?)
}

#[cfg(target_os = "linux")]
pub fn apply_sandbox(program: &BpfProgram) -> anyhow::Result<()> {
    seccompiler::apply_filter(program)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn create_plugin_sandbox() -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn apply_sandbox(_program: &()) -> anyhow::Result<()> {
    Ok(())
}
