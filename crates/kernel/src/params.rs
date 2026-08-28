//! Named document parameters and reference substitution.
//!
//! Features may use parameter names (strings) anywhere a numeric literal is
//! allowed. Before execution, references are resolved from `CadDocument::parameters`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ir::ValidationError;

/// Keys whose string values must never be treated as parameter references.
const SKIP_STRING_KEYS: &[&str] = &[
    "op",
    "bodyId",
    "documentId",
    "name",
    "units",
    "plane",
    "axis",
    "kind",
    "scope",
    "face",
    "edges",
    "target",
    "role",
    "say",
    "id",
];

/// Replace `"plate_width"` / `"$plate_width"` string leaves with numeric values
/// from `parameters`. Mutates `value` in place (typically the whole document JSON).
pub fn substitute_refs(value: &mut Value, parameters: &BTreeMap<String, f64>) -> Result<(), String> {
    if parameters.is_empty() {
        return Ok(());
    }
    substitute_node(value, parameters, None);
    Ok(())
}

fn substitute_node(node: &mut Value, parameters: &BTreeMap<String, f64>, key: Option<&str>) {
    match node {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                substitute_node(v, parameters, Some(k.as_str()));
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                substitute_node(item, parameters, key);
            }
        }
        Value::String(s) => {
            if key.is_some_and(|k| SKIP_STRING_KEYS.contains(&k)) {
                return;
            }
            let name = s.strip_prefix('$').unwrap_or(s.as_str());
            if let Some(&val) = parameters.get(name) {
                if let Some(n) = serde_json::Number::from_f64(val) {
                    *node = Value::Number(n);
                }
            }
        }
        _ => {}
    }
}

/// Validate the parameter table itself.
pub fn validate_parameters(parameters: &BTreeMap<String, f64>) -> Result<(), ValidationError> {
    for (name, value) in parameters {
        if name.trim().is_empty() {
            return Err(ValidationError::InvalidParameter {
                index: 0,
                message: "parameter name must not be empty".into(),
            });
        }
        if !value.is_finite() || *value <= 0.0 {
            return Err(ValidationError::InvalidParameter {
                index: 0,
                message: format!("parameter '{name}' must be a positive finite number"),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_box_size_refs() {
        let mut doc = json!({
            "parameters": { "w": 80.0, "d": 40.0, "t": 10.0 },
            "units": "mm",
            "bodies": [{
                "bodyId": "b",
                "features": [{ "op": "box", "size": ["w", "d", "t"] }]
            }]
        });
        let params: BTreeMap<String, f64> =
            serde_json::from_value(doc["parameters"].clone()).unwrap();
        substitute_refs(&mut doc, &params).unwrap();
        assert_eq!(doc["bodies"][0]["features"][0]["size"], json!([80.0, 40.0, 10.0]));
    }

    #[test]
    fn does_not_substitute_body_id() {
        let mut doc = json!({
            "parameters": { "body_main": 99.0 },
            "bodies": [{ "bodyId": "body_main", "features": [] }]
        });
        let params: BTreeMap<String, f64> =
            serde_json::from_value(doc["parameters"].clone()).unwrap();
        substitute_refs(&mut doc, &params).unwrap();
        assert_eq!(doc["bodies"][0]["bodyId"], "body_main");
    }
}
