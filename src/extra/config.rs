use std::collections::HashMap;

use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub target: TargetSection,
    pub meta: Option<MetaSection>,
    pub defines: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct TargetSection {
    pub arch: String,
    pub output: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct MetaSection {
    pub name: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
}

pub fn load_config(path: &str) -> ProjectConfig {
    let fallback = r#"[target]
    arch = "arm32"
    output = "build/main.s"
    "#;
 
    let content = std::fs::read_to_string(path).unwrap_or_else(|_| fallback.to_string());
    toml::from_str(&content).unwrap_or_else(|_| panic!("Invalid TOML format in: {}", path))
}
