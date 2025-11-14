use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{fs, path::Path};

pub mod smart_contract;
pub mod types;

pub fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted = Map::new();
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize_json(&map[&k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

pub fn stable_stringify<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let v = serde_json::to_value(value)?;
    let c = canonicalize_json(&v);
    Ok(serde_json::to_string(&c)?)
}

pub fn choose_k<'a, T: Clone>(items: &'a [T], k: usize) -> Vec<T> {
    let mut rng = rand::rng();
    let mut shuffled = items.to_vec();
    shuffled.shuffle(&mut rng);
    shuffled.into_iter().take(k).collect()
}

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

pub fn load_config_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Config> {
    let s = fs::read_to_string(path)?;
    let cfg: Config = toml::from_str(&s)?;
    Ok(cfg)
}

fn default_slot_ms() -> u64 {
    5000
}
