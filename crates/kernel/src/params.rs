//! Named document parameters, expressions, and slider-driven rewrites.
//!
//! Features may use parameter names (strings) or simple arithmetic
//! (`"bolt_length - head_height"`) anywhere a numeric literal is allowed.

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
    "hand",
];

/// Replace `"plate_width"` / `"$plate_width"` / `"a - b"` string leaves with numbers.
pub fn substitute_refs(
    value: &mut Value,
    parameters: &BTreeMap<String, f64>,
) -> Result<(), String> {
    if parameters.is_empty() {
        return Ok(());
    }
    substitute_node(value, parameters, None)?;
    Ok(())
}

fn substitute_node(
    node: &mut Value,
    parameters: &BTreeMap<String, f64>,
    key: Option<&str>,
) -> Result<(), String> {
    match node {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                substitute_node(v, parameters, Some(k.as_str()))?;
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                substitute_node(item, parameters, key)?;
            }
        }
        Value::String(s) => {
            if key.is_some_and(|k| SKIP_STRING_KEYS.contains(&k)) {
                return Ok(());
            }
            let name = s.strip_prefix('$').unwrap_or(s.as_str()).trim();
            if let Some(&val) = parameters.get(name) {
                if let Some(n) = serde_json::Number::from_f64(val) {
                    *node = Value::Number(n);
                }
                return Ok(());
            }
            if looks_like_expr(name) {
                match eval_expr(name, parameters) {
                    Ok(val) => {
                        if let Some(n) = serde_json::Number::from_f64(val) {
                            *node = Value::Number(n);
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn looks_like_expr(s: &str) -> bool {
    s.chars().any(|c| matches!(c, '+' | '-' | '*' | '/' | '('))
        && s.chars()
            .any(|c| c.is_ascii_alphabetic() || c.is_ascii_digit())
}

/// Rewrite numeric feature fields so a parameter slider actually changes geometry
/// even when the agent baked literals instead of `"head_width"` refs.
pub fn apply_parameter_delta(value: &mut Value, name: &str, old: f64, new: f64) {
    if !old.is_finite() || !new.is_finite() || (old - new).abs() < 1e-12 || old.abs() < 1e-12 {
        return;
    }
    let mut replaced_exact = false;
    rewrite_numbers(value, None, old, new, &mut replaced_exact);
    if is_axial_overall_name(name) && !replaced_exact {
        bump_largest_axial_field(value, new - old);
    }
}

fn rewrite_numbers(
    node: &mut Value,
    key: Option<&str>,
    old: f64,
    new: f64,
    replaced_exact: &mut bool,
) {
    match node {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                rewrite_numbers(v, Some(k.as_str()), old, new, replaced_exact);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                rewrite_numbers(item, key, old, new, replaced_exact);
            }
        }
        Value::Number(n) => {
            if key.is_some_and(|k| {
                matches!(
                    k,
                    "op" | "bodyId"
                        | "documentId"
                        | "name"
                        | "units"
                        | "plane"
                        | "axis"
                        | "kind"
                        | "id"
                        | "hand"
                )
            }) {
                return;
            }
            let Some(v) = n.as_f64() else {
                return;
            };
            if let Some(nv) = scaled_like(v, old, new, key) {
                if (v - old).abs() <= number_tol(old) {
                    *replaced_exact = true;
                }
                if let Some(num) = serde_json::Number::from_f64(nv) {
                    *node = Value::Number(num);
                }
            }
        }
        _ => {}
    }
}

fn number_tol(old: f64) -> f64 {
    (old.abs() * 0.002).max(0.02)
}

/// Match v ≈ ± old·ratio for common hex / centered-profile ratios.
fn scaled_like(v: f64, old: f64, new: f64, key: Option<&str>) -> Option<f64> {
    let tol = number_tol(old);
    let axial = key.is_some_and(|k| matches!(k, "length" | "depth" | "height"));
    // Hex vertex ratios only on coordinates / widths, never on axial feature lengths
    // (M8 shank 34.7 ≈ 40·√3/2, which is a coincidence we must not rewrite).
    let ratios: &[f64] = if axial {
        &[1.0, 0.5]
    } else {
        &[
            1.0,
            0.5,
            1.0 / 3.0_f64.sqrt(),
            3.0_f64.sqrt() / 2.0,
            2.0 / 3.0_f64.sqrt(),
            0.25,
        ]
    };
    let sign = if v < 0.0 { -1.0 } else { 1.0 };
    let mag = v.abs();
    for r in ratios {
        if (mag - old.abs() * r).abs() <= tol.max(old.abs() * r * 0.002) {
            return Some(sign * new.abs() * r);
        }
    }
    None
}

fn is_axial_overall_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "length" | "height" | "bolt_length" | "overall_length" | "total_length"
    ) || (n.ends_with("_length") && !n.contains("head") && !n.contains("pitch"))
}

fn bump_largest_axial_field(node: &mut Value, delta: f64) {
    let mut best_val = 0.0_f64;
    find_max_axial(node, None, &mut best_val);
    if best_val <= 0.05 {
        return;
    }
    apply_axial_bump(node, None, best_val, delta);
}

fn find_max_axial(node: &Value, key: Option<&str>, best: &mut f64) {
    match node {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "parameters" {
                    continue;
                }
                find_max_axial(v, Some(k.as_str()), best);
            }
        }
        Value::Number(n) => {
            if let Some(v) = n.as_f64() {
                if key.is_some_and(|k| matches!(k, "length" | "depth" | "height")) && v > *best {
                    *best = v;
                }
            }
        }
        Value::Array(arr) => {
            for v in arr {
                find_max_axial(v, key, best);
            }
        }
        _ => {}
    }
}

fn apply_axial_bump(node: &mut Value, key: Option<&str>, target: f64, delta: f64) {
    match node {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                apply_axial_bump(v, Some(k.as_str()), target, delta);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                apply_axial_bump(v, key, target, delta);
            }
        }
        Value::Number(n) => {
            if key.is_some_and(|k| matches!(k, "length" | "depth" | "height")) {
                if let Some(v) = n.as_f64() {
                    if (v - target).abs() < 1e-9 {
                        let nv = (v + delta).max(0.05);
                        if let Some(num) = serde_json::Number::from_f64(nv) {
                            *node = Value::Number(num);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

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

// ── Tiny expression evaluator: + - * / ( ) names numbers ─────────────────────

fn eval_expr(src: &str, parameters: &BTreeMap<String, f64>) -> Result<f64, String> {
    let tokens = tokenize(src)?;
    let mut p = Parser {
        tokens,
        pos: 0,
        parameters,
    };
    let v = p.parse_add()?;
    if p.pos != p.tokens.len() {
        return Err(format!("unexpected token in expression '{src}'"));
    }
    Ok(v)
}

#[derive(Clone, Debug)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
}

struct Parser<'a> {
    tokens: Vec<Tok>,
    pos: usize,
    parameters: &'a BTreeMap<String, f64>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos)
    }
    fn bump(&mut self) -> Option<Tok> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }
    fn parse_add(&mut self) -> Result<f64, String> {
        let mut v = self.parse_mul()?;
        while let Some(Tok::Op(op)) = self.peek() {
            if *op != '+' && *op != '-' {
                break;
            }
            let op = *op;
            self.bump();
            let r = self.parse_mul()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Ok(v)
    }
    fn parse_mul(&mut self) -> Result<f64, String> {
        let mut v = self.parse_unary()?;
        while let Some(Tok::Op(op)) = self.peek() {
            if *op != '*' && *op != '/' {
                break;
            }
            let op = *op;
            self.bump();
            let r = self.parse_unary()?;
            v = if op == '*' {
                v * r
            } else {
                if r.abs() < 1e-18 {
                    return Err("division by zero in parameter expression".into());
                }
                v / r
            };
        }
        Ok(v)
    }
    fn parse_unary(&mut self) -> Result<f64, String> {
        if let Some(Tok::Op('-')) = self.peek() {
            self.bump();
            return Ok(-self.parse_unary()?);
        }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> Result<f64, String> {
        match self.bump() {
            Some(Tok::Num(n)) => Ok(n),
            Some(Tok::Ident(name)) => self
                .parameters
                .get(&name)
                .copied()
                .ok_or_else(|| format!("unknown parameter '{name}' in expression")),
            Some(Tok::Op('(')) => {
                let v = self.parse_add()?;
                match self.bump() {
                    Some(Tok::Op(')')) => Ok(v),
                    _ => Err("missing ')' in parameter expression".into()),
                }
            }
            other => Err(format!("expected number or name, got {other:?}")),
        }
    }
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit()
            || (c == '.' && i + 1 < b.len() && (b[i + 1] as char).is_ascii_digit())
        {
            let start = i;
            i += 1;
            while i < b.len() && ((b[i] as char).is_ascii_digit() || b[i] as char == '.') {
                i += 1;
            }
            let n: f64 = src[start..i]
                .parse()
                .map_err(|_| format!("bad number in '{src}'"))?;
            out.push(Tok::Num(n));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            i += 1;
            while i < b.len() {
                let d = b[i] as char;
                if d.is_ascii_alphanumeric() || d == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Tok::Ident(src[start..i].to_string()));
            continue;
        }
        if matches!(c, '+' | '-' | '*' | '/' | '(' | ')') {
            out.push(Tok::Op(c));
            i += 1;
            continue;
        }
        return Err(format!("unexpected '{c}' in expression '{src}'"));
    }
    Ok(out)
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
        assert_eq!(
            doc["bodies"][0]["features"][0]["size"],
            json!([80.0, 40.0, 10.0])
        );
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

    #[test]
    fn evaluates_subtraction_expr() {
        let params = BTreeMap::from([("bolt_length".into(), 40.0), ("head_height".into(), 5.3)]);
        assert!((eval_expr("bolt_length - head_height", &params).unwrap() - 34.7).abs() < 1e-9);
    }

    #[test]
    fn delta_scales_hex_half_width() {
        let mut doc = json!({
            "bodies": [{
                "features": [{
                    "op": "fuse",
                    "depth": 5.3,
                    "profile": { "polyline": { "points": [[7.506, 0], [3.753, 6.5]] } }
                }]
            }]
        });
        apply_parameter_delta(&mut doc, "head_width", 13.0, 26.0);
        let y = doc["bodies"][0]["features"][0]["profile"]["polyline"]["points"][1][1]
            .as_f64()
            .unwrap();
        assert!((y - 13.0).abs() < 0.05, "y={y}");
        let depth = doc["bodies"][0]["features"][0]["depth"].as_f64().unwrap();
        assert!((depth - 5.3).abs() < 1e-6, "depth should stay {depth}");
    }

    #[test]
    fn delta_length_bumps_thread_length() {
        let mut doc = json!({
            "bodies": [{
                "features": [
                    { "op": "thread", "length": 34.7, "size": "M8" },
                    { "op": "fuse", "depth": 5.3 }
                ]
            }]
        });
        apply_parameter_delta(&mut doc, "bolt_length", 40.0, 50.0);
        let len = doc["bodies"][0]["features"][0]["length"].as_f64().unwrap();
        assert!((len - 44.7).abs() < 0.05, "length={len}");
    }

    #[test]
    fn bolt_shank_expressions_eval() {
        let mut doc = json!({
            "parameters": { "bolt_length": 40.0, "head_height": 5.5 },
            "bodies": [{ "features": [
                { "op": "cylinder", "height": "bolt_length - head_height + 1",
                  "at": [0, 0, "head_height - 1"] },
                { "op": "thread", "length": "bolt_length - head_height" }
            ] }]
        });
        substitute_refs(
            &mut doc,
            &BTreeMap::from([("bolt_length".into(), 40.0), ("head_height".into(), 5.5)]),
        )
        .unwrap();
        assert!((doc["bodies"][0]["features"][0]["height"].as_f64().unwrap() - 35.5).abs() < 1e-9);
        assert!((doc["bodies"][0]["features"][0]["at"][2].as_f64().unwrap() - 4.5).abs() < 1e-9);
        assert!((doc["bodies"][0]["features"][1]["length"].as_f64().unwrap() - 34.5).abs() < 1e-9);
    }
}
