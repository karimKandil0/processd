use std::collections::{HashMap, VecDeque};
use crate::config::SystemConfig;
use crate::error::ConfigError;

pub struct DependencyGraph {
    pub edges: HashMap<String, Vec<String>>,
}

pub fn build_dependency_graph(config: &SystemConfig) -> Result<DependencyGraph, ConfigError> {
    // Map capability name -> service name
    let mut provides_map: HashMap<&str, &str> = HashMap::new();
    for (name, svc) in &config.service {
        for cap in &svc.provides {
            if let Some(existing) = provides_map.get(cap.as_str()) {
                return Err(ConfigError::DuplicateProvider {
                    capability: cap.clone(),
                    existing: existing.to_string(),
                    new: name.clone(),
                });
            }
            provides_map.insert(cap.as_str(), name.as_str());
        }
    }

    // Map wants -> service names
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    for (name, svc) in &config.service {
        let mut deps = Vec::new();
        for want in &svc.wants {
            match provides_map.get(want.as_str()) {
                Some(provider) => deps.push(provider.to_string()),
                None => return Err(ConfigError::UnknownDependency {
                    service: name.clone(),
                    dep: want.clone(),
                }),
            }
        }
        edges.insert(name.clone(), deps);
    }

    // Cycle detection with DFS
    #[derive(PartialEq)]
    enum State {Unvisited, InProgress, Done}
    let mut states: HashMap<&str, State> = config.service.keys()
        .map(|k| (k.as_str(), State::Unvisited))
        .collect();

    fn dfs<'a>(
        node: &'a str,
        edges: &'a HashMap<String, Vec<String>>,
        states: &mut HashMap<&'a str, State>,
    ) -> Result<(), ConfigError> {
        states.insert(node, State::InProgress);
        for dep in edges.get(node).into_iter().flatten() {
            match states.get(dep.as_str()) {
                Some(State::InProgress) => return Err(ConfigError::CycleDetected(dep.clone())),
                Some(State::Done) => {}
                _ => dfs(dep.as_str(), edges, states)?,
            }
        }
        states.insert(node, State::Done);
        Ok(())
    }

    let nodes: Vec<&str> = config.service.keys().map(|k| k.as_str()).collect();
    for node in nodes {
        if states.get(node) == Some(&State::Unvisited) {
            dfs(node, &edges, &mut states)?;
        }
    }

    Ok(DependencyGraph { edges })
}

pub fn topological_sort(graph: &DependencyGraph) -> Vec<String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    for (node, deps) in &graph.edges {
        in_degree.entry(node.as_str()).or_insert(0);
        for dep in deps {
            *in_degree.entry(dep.as_str()).or_insert(0) += 1;
        }
    }

    let mut queue: VecDeque<&str> = in_degree.iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&node, _)| node)
        .collect();

    // sort for determinism
    let mut queue_vec: Vec<&str> = queue.drain(..).collect();
    queue_vec.sort();
    queue = queue_vec.into();

    let mut result = Vec::new();
    while let Some(node) = queue.pop_front() {
        result.push(node.to_string());
        for dep in graph.edges.get(node).into_iter().flatten() {
            let deg = in_degree.get_mut(dep.as_str()).unwrap();
            *deg -= 1;
            if *deg == 0 {
                queue.push_back(dep.as_str());
            }
        }
    }
    result.reverse();
    result
}
