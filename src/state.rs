use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Generic state file manager for key-value persistence.
///
/// Stores state as simple `KEY=VALUE` lines in a file.
pub struct StateFile {
    path: PathBuf,
}

impl StateFile {
    /// Create a new StateFile manager for the given file path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Load a single value by key from the state file.
    pub fn load_value(&self, key: &str) -> Option<String> {
        if !self.path.exists() {
            return None;
        }
        if let Ok(contents) = fs::read_to_string(&self.path) {
            for line in contents.lines() {
                if let Some(val) = line.strip_prefix(&format!("{}=", key)) {
                    return Some(val.trim().to_string());
                }
            }
        }
        None
    }

    /// Save a single key-value pair to the state file, preserving other values.
    pub fn save_value(&self, key: &str, value: &str) -> Result<()> {
        let mut state = self.load_all();
        state.insert(key.to_string(), value.to_string());
        self.save_all(&state)
    }

    /// Load all key-value pairs from the state file.
    pub fn load_all(&self) -> HashMap<String, String> {
        let mut state = HashMap::new();
        if self.path.exists() {
            if let Ok(contents) = fs::read_to_string(&self.path) {
                for line in contents.lines() {
                    if let Some((k, v)) = line.split_once('=') {
                        state.insert(k.to_string(), v.trim().to_string());
                    }
                }
            }
        }
        state
    }

    /// Save all key-value pairs to the state file (sorted by key for consistency).
    pub fn save_all(&self, state: &HashMap<String, String>) -> Result<()> {
        let keys: Vec<_> = state.keys().collect();
        let mut content = String::new();
        for k in keys {
            content.push_str(&format!("{}={}\n", k, state[k]));
        }
        fs::write(&self.path, content)?;
        Ok(())
    }

    /// Delete the state file.
    pub fn delete(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)?;
        }
        Ok(())
    }

    /// Check if the state file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_save_and_load_value() {
        let temp_file = "test_state_1.env";
        let state = StateFile::new(temp_file);

        // Save a value
        state.save_value("KEY1", "value1").unwrap();
        assert_eq!(state.load_value("KEY1"), Some("value1".to_string()));

        // Clean up
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_load_value_nonexistent() {
        let state = StateFile::new("nonexistent.env");
        assert_eq!(state.load_value("KEY1"), None);
    }

    #[test]
    fn test_save_multiple_values() {
        let temp_file = "test_state_2.env";
        let state = StateFile::new(temp_file);

        state.save_value("KEY1", "value1").unwrap();
        state.save_value("KEY2", "value2").unwrap();

        assert_eq!(state.load_value("KEY1"), Some("value1".to_string()));
        assert_eq!(state.load_value("KEY2"), Some("value2".to_string()));

        // Clean up
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_load_all() {
        let temp_file = "test_state_3.env";
        let state = StateFile::new(temp_file);

        state.save_value("KEY1", "value1").unwrap();
        state.save_value("KEY2", "value2").unwrap();

        let all = state.load_all();
        assert_eq!(all.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(all.get("KEY2"), Some(&"value2".to_string()));

        // Clean up
        fs::remove_file(temp_file).unwrap();
    }

    #[test]
    fn test_delete() {
        let temp_file = "test_state_4.env";
        let state = StateFile::new(temp_file);

        state.save_value("KEY1", "value1").unwrap();
        assert!(state.exists());

        state.delete().unwrap();
        assert!(!state.exists());
    }
}
