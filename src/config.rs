use std::collections::BTreeMap;

#[derive(clap::ValueEnum, Clone, Debug, PartialEq, Eq)]
pub enum ServiceLevel {
    User,
    System,
}

impl Default for ServiceLevel {
    fn default() -> Self {
        ServiceLevel::System
    }
}

#[derive(Clone, Default, Debug)]
pub struct ServiceConfig {
    pub name: String,
    pub command: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub level: ServiceLevel,
}
