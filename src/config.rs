//! Project configuration, loaded from `project.comfx` (TOML).

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub target: TargetSection,
}

#[derive(Debug, Deserialize)]
pub struct TargetSection {
    pub arch: String,
    pub output: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            target: TargetSection {
                arch: "arm32".to_string(),
                output: None,
            },
        }
    }
}

pub fn load_config(path: &str) -> ProjectConfig {
    match std::fs::read_to_string(path) {
        Ok(content) => toml::from_str(&content)
            .unwrap_or_else(|e| panic!("Invalid TOML format in {}: {}", path, e)),
        Err(_) => ProjectConfig::default(),
    }
}
