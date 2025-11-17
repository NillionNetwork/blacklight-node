use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ElectionConfig {
    pub validators_per_htx: usize,
    pub approve_threshold: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub election: ElectionConfig,
    #[serde(default = "default_slot_ms")]
    pub slot_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            election: ElectionConfig {
                validators_per_htx: 3,
                approve_threshold: 2,
            },
            slot_ms: default_slot_ms(),
        }
    }
}

/// Load configuration from a TOML file.
pub fn load_config_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let s = fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&s)?;
    Ok(cfg)
}

fn default_slot_ms() -> u64 {
    5000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.slot_ms, 5000);
        assert_eq!(config.election.validators_per_htx, 3);
        assert_eq!(config.election.approve_threshold, 2);
    }
}
