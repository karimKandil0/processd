use nix::mount::{mount, MsFlags};
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::sys::signal::{sigprocmask, SigSet, SigmaskHow, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{execve, Pid};
use std::ffi::CString;

const TOKEN_SIGNAL: u64 = 1;

fn main() {
    eprintln!("[processd] starting");

    // mount virtual filesystems
    if let Err(e) = mount_virtual_filesystems() {
        eprintln!("[processd] mount failed: {e}");
            emergency_shell();
    };

    let mut sfd = match setup_signalfd() {
        Ok(fd) => fd,
        Err(e) => {
            eprintln!("[processd] signalfd failed: {e}");
            emergency_shell();
        }
    };

    let epoll = match setup_epoll(&sfd) {
        Ok(ep) => ep,
        Err(e) => { eprintln!("[processd] epoll failed: {e}"); emergency_shell(); }
    };

    eprintln!("[processd] running...");

    let mut events = [EpollEvent::empty(); 8];
    loop {
        match epoll.wait(&mut events, None::<u16>) {
            Ok(n) => {
                for event in &events[..n] {
                    if event.data() == TOKEN_SIGNAL {
                        handle_signals(&mut sfd);
                    }
                }
            }
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e)                 => { eprintln!("[processd] epoll_wait error: {e}"); break; }
        }
    }
}

fn mount_virtual_filesystems() -> Result<(), nix::Error> {
    for (src, target, fstype) in [
        ("proc",     "/proc", "proc"),
        ("sysfs",    "/sys",  "sysfs"),
        ("devtmpfs", "/dev",  "devtmpfs"),
    ] {
        match mount(Some(src), target, Some(fstype), MsFlags::empty(), None::<&str>) {
            Ok(())                        => {}
            Err(nix::errno::Errno::EBUSY) => {}
            Err(nix::errno::Errno::EPERM) => {}
            Err(e)                        => return Err(e),
        }
    }
    Ok(())
}

fn emergency_shell() -> ! {
    eprintln!("[processd] FATAL: dropping to emergency shell");
    let shell = CString::new("/bin/sh").unwrap();
    let args: &[CString] = &[shell.clone()];
    let env:  &[CString] = &[];
    let _ = execve(&shell, args, env);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

fn setup_signalfd() -> Result<SignalFd, nix::Error> {
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGCHLD);
    mask.add(Signal::SIGTERM);
    mask.add(Signal::SIGHUP);

    sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;

    SignalFd::with_flags(&mask, SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK)
}

fn setup_epoll(sfd: &SignalFd) -> Result<Epoll, nix::Error> {
    let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
    epoll.add(sfd, EpollEvent::new(EpollFlags::EPOLLIN, TOKEN_SIGNAL))?;
    Ok(epoll)
}

fn reap_zombies() {
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code))     => eprintln!("[processd] pid {pid} exited with code {code}"),
            Ok(WaitStatus::Signaled(pid, sig, _)) => eprintln!("[processd] pid {pid} terminated by signal {sig}"),
            Ok(WaitStatus::StillAlive)             => break,
            Ok(_)                                  => continue,
            Err(nix::errno::Errno::ECHILD)         => break,
            Err(e)                                 => { eprintln!("[processd] waitpid error: {e}"); break; }
        }
    }
}

fn handle_signals(sfd: &mut SignalFd) {
    loop {
        match sfd.read_signal() {
            Ok(Some(info)) => match Signal::try_from(info.ssi_signo as i32) {
                Ok(Signal::SIGCHLD) => reap_zombies(),
                Ok(Signal::SIGTERM) => {
                    eprintln!("[processd] shutting down...");
                    std::process::exit(0);
                },
                Ok(Signal::SIGHUP)  => eprintln!("[processd] SIGHUP (reload not implemented yet)"),
                _                   => {}
            },
            Ok(None)       => break,
            Err(e)         => { eprintln!("[processd] signalfd read error: {e}"); break;}
        }
    }
}
