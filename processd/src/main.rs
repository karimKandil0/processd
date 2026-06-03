use nix::mount::{mount, MsFlags};
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::sys::signal::{sigprocmask, SigSet, SigmaskHow, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execve, Pid};
use std::ffi::CString;

const TOKEN_SIGNAL: u64 = 1;

fn main() {}

fn mount_virtual_filesystems() -> Result<(), nix::Error> {
    mount(Some("proc"),     "/proc", Some("proc"),     MsFlags::empty(), None::<&str>)?;
    mount(Some("sysfs"),    "/sys",  Some("sysfs"),    MsFlags::empty(), None::<&str>)?;
    mount(Some("devtmpfs"), "/dev",  Some("devtmpfs"), MsFlags::empty(), None::<&str>)?;
    Ok(())
}

fn emergency_shell() -> ! {
    eprintln!("[processd] FATAL: dropping to emergency shell");
    let shell = CString::new("/bin/sh").unwrap();
    let _ = execve(&shell, &[shell.clone()], &[] as &[CString]);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn setup_signalfd() -> Result<SignalFd, nix::Error> {
    todo!()
}

fn setup_epoll(sfd: &SignalFd) -> Result<Epoll, nix::Error> {
    todo!()
}

fn reap_zombies() {
    todo!()
}

fn handle_signals(sfd: &mut SignalFd) {
    todo!()
}
