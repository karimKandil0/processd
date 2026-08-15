mod supervisor;

use processd_core::{parse_config, build_dependency_graph, topological_sort, diff, Action,};
use supervisor::{ProcessTable, spawn_service};
use std::path::Path;
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

    let config = match parse_config(Path::new("/etc/processd/system.toml")) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("[processd] dependency error: {e}");
            emergency_shell();
        }
    };

    let graph = match build_dependency_graph(&config) {
        Ok(g)  => g,
        Err(e) => {
            eprintln!("[processd] dependency error: {e}");
            emergency_shell();
        }
    };

    let _order = topological_sort(&graph);
    let mut table = ProcessTable::new();

    for (name, svc) in &config.service {
        table.register(name, svc.clone());
    }

    let actions = diff(&config, &table.snapshot());
    apply(&actions, &mut table);

    let mut events = [EpollEvent::empty(); 8];
    loop {
        match epoll.wait(&mut events, None::<u16>) {
            Ok(n) => {
                for event in &events[..n] {
                    if event.data() == TOKEN_SIGNAL {
                        let _ = handle_signals(&mut sfd, &mut table);
                        let actions = diff(&config, &table.snapshot());
                        apply(&actions, &mut table);
                    }
                }
            }
            Err(nix::errno::Errno::EINTR) => continue,
            Err(e)                 => { eprintln!("[processd] epoll_wait error: {e}"); break; }
        }
    }
}

fn apply(actions: &[Action], table: &mut ProcessTable) {
    for action in actions {
        match action {
            Action::Start(name) => {
                let cfg = table.services[name].config.clone();
                match spawn_service(name, &cfg) {
                    Ok(pid) => {
                        eprintln!("[processd] spawned {name} (pid {pid})");
                        table.record_spawn(name, pid);
                    }
                    Err(e) => eprintln!("[processd] failed to spawn {name}: {e}"),
                }
            }
            Action::Stop(name) => {
                eprintln!("[processd] stop {name}: not yet implemented");
            }
            Action::Restart(name) => {
                eprintln!("[processd] restart {name}: not yet implemented");
            }
            Action::NoOp() => {},
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

fn reap_zombies(table: &mut ProcessTable) -> Vec<String> {
    let mut to_respawn = Vec::new();
    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) => {
                eprintln!("[processd] pid {pid} exited with code {code}");
                if let Some(name) = table.handle_death(pid, WaitStatus::Exited(pid, code)) {
                    to_respawn.push(name);
                }
            }
            Ok(WaitStatus::Signaled(pid, sig, dumped)) => {
                eprintln!("[processd] pid {pid} terminated by signal {sig}");
                if let Some(name) = table.handle_death(pid, WaitStatus::Signaled(pid, sig, dumped)) {
                    to_respawn.push(name);
                }
            }
            Ok(WaitStatus::StillAlive)     => break,
            Ok(_)                          => continue,
            Err(nix::errno::Errno::ECHILD) => break,
            Err(e)                         => { eprintln!("[processd] waitpid error: {e}"); break; }
        }
    }
    to_respawn
}

fn handle_signals(sfd: &mut SignalFd, table: &mut ProcessTable) -> Vec<String> {
    let mut to_respawn = Vec::new();
    loop {
        match sfd.read_signal() {
            Ok(Some(info)) => match Signal::try_from(info.ssi_signo as i32) {
                Ok(Signal::SIGCHLD) => to_respawn.extend(reap_zombies(table)),
                Ok(Signal::SIGTERM) => {
                    eprintln!("[processd] shutting down...");
                    std::process::exit(0);
                },
                Ok(Signal::SIGHUP)  => eprintln!("[processd] SIGHUP (reload not implemented yet)"),
                _                   => {}
            },
            Ok(None)       => break,
            Err(e)         => { eprintln!("[processd] signalfd read error: {e}"); break; }
        }
    }
    to_respawn
}
