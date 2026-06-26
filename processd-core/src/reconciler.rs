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
