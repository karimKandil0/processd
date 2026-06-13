use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("unknown dependency '{dep}' required by '{service}'")]
    UnknownDependency { service: String, dep: String },

    #[error("dependency cycle detected involving '{0}'")]
    CycleDetected(String),

    #[error("capability '{capability}' already provided by '{existing}', cannot also be provided by '{new}'")]
    DuplicateProvider { capability: String, existing: String, new: String },
}


