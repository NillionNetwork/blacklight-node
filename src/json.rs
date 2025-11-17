use serde::Serialize;
use serde_json::{Map, Value};

/// Canonicalize a JSON value by recursively sorting all object keys.
/// This ensures deterministic serialization for hashing and comparison.
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

/// Serialize a value to a stable JSON string with sorted keys.
/// Useful for deterministic hashing and comparison.
pub fn stable_stringify<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let v = serde_json::to_value(value)?;
    let c = canonicalize_json(&v);
    Ok(serde_json::to_string(&c)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_canonicalize_json() {
        let input = json!({
            "z": 1,
            "a": 2,
            "m": {
                "y": 3,
                "b": 4
            }
        });

        let canonical = canonicalize_json(&input);
        let stringified = serde_json::to_string(&canonical).unwrap();

        // Keys should be sorted at all levels
        assert!(stringified.starts_with(r#"{"a":2"#));
    }

    #[test]
    fn test_stable_stringify() {
        #[derive(serde::Serialize)]
        struct Test {
            z: i32,
            a: i32,
        }

        let obj = Test { z: 1, a: 2 };
        let result = stable_stringify(&obj).unwrap();

        // Should serialize with sorted keys
        assert!(result.contains("\"a\":2"));
        assert!(result.contains("\"z\":1"));
    }
}
