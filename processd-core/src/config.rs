use std::collections::HashMap;
use std::path::Path;
use serde::Deserialize;
use crate::error::ConfigError;

#[derive(Debug, Clone, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Always,
    #[default]
    OnFailure,
    Never,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
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
    let config = toml::from_str(&contents)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_tmp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_valid_config() {
       let toml = r#"
           [service.postgres]
           binary   = "/usr/bin/postgres"
           args     = ["-D", "/var/lib/postgres"]
           user     = "postgres"
           wants    = ["network"]
           provides = ["database"]
           restart  = "on-failure"
        "#;
       let f = write_tmp(toml);
       let cfg = parse_config(f.path()).unwrap();
       let pg = &cfg.service["postgres"];
       assert_eq!(pg.binary, "/usr/bin/postgres");
       assert_eq!(pg.args, vec!["-D", "/var/lib/postgres"]);
       assert_eq!(pg.user.as_deref(), Some("postgres"));
       assert_eq!(pg.wants, vec!["network"]);
       assert!(matches!(pg.restart, RestartPolicy::OnFailure));
    }

    #[test]
    fn parse_missing_binary() {
        let toml = r#"
            [service.broken]
            user = "nobody"
        "#;        
        let f = write_tmp(toml);
        let err = parse_config(f.path()).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::Toml(_)));
    }

    #[test]
    fn empty_config() {
        let f = write_tmp("");
        let cfg = parse_config(f.path()).unwrap();
        assert!(cfg.service.is_empty());
    }
}
