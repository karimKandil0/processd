use std::collections::HashMap;
use std::ffi::CString;
use std::time::Instant;
use nix::sys::wait::WaitStatus;
use nix::unistd::{execve, fork, setgid, setuid, ForkResult, Pid, User};
use processd_core::{RestartPolicy, ServiceConfig};

#[derive(Debug, thiserror::Error)]
pub enum SpawnError {
    #[error("syscall error:{0}")]
    Nix(#[from] nix::Error),

    #[error("invalid CString (null byte in arg): {0}")]
    InvalidCString(#[from] std::ffi::NulError),

    #[error("unknown user in config")]
    UnknownUser,
}

pub enum ServiceState {
    Stopped,
    Starting,
    Running { pid: Pid, attempts: u32 },
    Backoff { next_attempt_at: Instant, attempts: u32 },
    Failed { final_status: String },    
}

pub struct ServiceEntry {
    pub config: ServiceConfig,
    pub state:  ServiceState,
}

pub struct ProcessTable {
    pub services: HashMap<String, ServiceEntry>,
    pub pid_to_name: HashMap<Pid, String>,
}

impl ProcessTable {
    pub fn new() -> Self {
        Self {
            services: HashMap::new(),
            pid_to_name: HashMap::new(),
        }
    }

    pub fn register(&mut self, name: &str, config: ServiceConfig) {
        self.services.insert(
            name.to_string(),
            ServiceEntry { config, state: ServiceState::Stopped },
        );
    }

    pub fn record_spawn(&mut self, name: &str, pid: Pid) {
        if let Some(entry) = self.services.get_mut(name) {
            let attempts = match &entry.state {
                ServiceState::Backoff { attempts, .. } => *attempts,
                _ => 0,
            };
            entry.state = ServiceState::Running { pid, attempts };
            self.pid_to_name.insert(pid, name.to_string());
        }
    }

    pub fn lookup_pid(&self, pid: Pid) -> Option<&str> {
        self.pid_to_name.get(&pid).map(|s| s.as_str())
    }

    pub fn forget_pid(&mut self, pid: Pid) {
        self.pid_to_name.remove(&pid);
    }

    /// Called when a child process dies. Updates the table's state and
    /// returns the service name if it should be re-spawned, or None if not.
    /// Also returns None for orphaned PIDs we don't manage (Firefox case).
    pub fn handle_death(&mut self, pid: Pid, status: WaitStatus) -> Option<String> {
        let name = self.pid_to_name.remove(&pid)?;
        let entry = self.services.get_mut(&name)?;

        let exit_ok = matches!(status, WaitStatus::Exited(_, 0));

        let should_restart = match entry.config.restart {
            RestartPolicy::Always    => true,
            RestartPolicy::OnFailure => !exit_ok,
            RestartPolicy::Never     => false,
        };

        if should_restart {
            entry.state = ServiceState::Stopped;
            Some(name)
        } else if exit_ok {
            entry.state = ServiceState::Stopped;
            None
        } else {
            entry.state = ServiceState::Failed {
                final_status: format!("{:?}", status),
            };
            None
        }
    }
}


pub fn spawn_service(name: &str, cfg: &ServiceConfig) -> Result<Pid, SpawnError> {
    let binary = CString::new(cfg.binary.as_str())?;
    let mut argv: Vec<CString> = Vec::with_capacity(cfg.args.len() + 1);
    argv.push(binary.clone());
    for arg in &cfg.args {
        argv.push(CString::new(arg.as_str())?);
    }

    let envp: Vec<CString> = vec![];
    
    let user_info = match &cfg.user {
        Some(u) => Some(User::from_name(u)?.ok_or(SpawnError::UnknownUser)?),
        None    => None,
    };

    match unsafe { fork() }? {
        ForkResult::Parent { child } => Ok(child),

        ForkResult::Child => {
            if let Some(u) = user_info {
                let _ = setgid(u.gid);
                let _ = setuid(u.uid);
            }

            let _ = execve(&binary, &argv, &envp);

            unsafe { nix::libc::_exit(127) };
        }
    }

}
