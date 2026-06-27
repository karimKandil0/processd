use std::collections::{HashMap, HashSet};
use crate::config::{ServiceConfig, SystemConfig};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Start(String),
    Stop(String),
    Restart(String),
    NoOp(String),
}

pub struct ActualSnapshot {
    pub running: HashMap<String, ServiceConfig>,
    pub failed:  HashSet<String>,
}

pub fn diff(desired: &SystemConfig, actual: &ActualSnapshot) -> Vec<Action> {
    let mut actions = Vec::new();

    for (name, cfg) in &desired.service {
        let action = if actual.failed.contains(name) {
            Action::NoOp(name.clone())
        } else if actual.running.get(name) == Some(cfg) {
            Action::NoOp(name.clone())
        } else if actual.running.contains_key(name) {
            Action::Restart(name.clone())
        } else {
            Action::Start(name.clone())
        };
        actions.push(action);
    }

    for name in actual.running.keys() {
        if !desired.service.contains_key(name) {
            actions.push(Action::Stop(name.clone()));
        }
    }

    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RestartPolicy;

    fn svc(binary: &str) -> ServiceConfig {
        ServiceConfig {
            binary: binary.into(),
            args: vec![],
            user: None,
            wants: vec![],
            provides: vec![],
            restart: RestartPolicy::OnFailure,
        }
    }

    fn desired(services: &[(&str, ServiceConfig)]) -> SystemConfig {
        let mut map = HashMap::new();
        for (name, cfg) in services {
            map.insert(name.to_string(), cfg.clone());
        }
        SystemConfig { service: map }
    }

    fn snapshot(running: &[(&str, ServiceConfig)], failed: &[&str]) -> ActualSnapshot {
        ActualSnapshot {
            running: running.iter().map(|(n, c)| (n.to_string(), c.clone())).collect(),
            failed:  failed.iter().map(|n| n.to_string()).collect(),
        }
    }

    #[test]
    fn start_for_new_service() {
        let d = desired(&[("api", svc("/api"))]);
        let a = snapshot(&[], &[]);
        assert_eq!(diff(&d, &a), vec![Action::Start("api".into())]);
    }

    #[test]
    fn noop_for_running_same_config() {
        let cfg = svc("/api");
        let d = desired(&[("api", cfg.clone())]);
        let a = snapshot(&[("api", cfg)], &[]);
        assert_eq!(diff(&d, &a), vec![Action::NoOp("api".into())]);
    }

    #[test]
    fn restart_when_config_changes() {
        let d = desired(&[("api", svc("/new"))]);
        let a = snapshot(&[("api", svc("/old"))], &[]);
        assert_eq!(diff(&d, &a), vec![Action::Restart("api".into())]);
    }

    #[test]
    fn stop_for_removed_service() {
        let d = desired(&[]);
        let a = snapshot(&[("api", svc("/api"))], &[]);
        assert_eq!(diff(&d, &a), vec![Action::Stop("api".into())]);
    }

    #[test]
    fn noop_for_failed_service() {
        let d = desired(&[("api", svc("/api"))]);
        let a = snapshot(&[], &["api"]);
        assert_eq!(diff(&d, &a), vec![Action::NoOp("api".into())]);
    }
}
