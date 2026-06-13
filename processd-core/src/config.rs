use std::collections::HashMap;
use std::path::Path;
use serde::Deserialize;
use crate::error::ConfigError;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Always,
    #[default]
    OnFailure,
    Never,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceConfig {
    pub binary: String,
    #[serde(default)] pub args: Vec<String>,
    pub user: Option<String>,
    #[serde(default)] pub wants: Vec<String>,
    #[serde(default)] pub provides: Vec<String>,
    #[serde(default)] pub restart: RestartPolicy,
}

#[derive(Debug, Deserialize)]
pub struct SystemConfig {
    #[serde(default)] pub service: HashMap<String, ServiceConfig>,
}

pub fn parse_config(path: &Path) -> Result<SystemConfig, ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    let cofnig = toml::from_str(&contents)?;
    Ok(config)
}
