//! Named document parameters, expressions, and slider-driven rewrites.
//!
//! Features may use parameter names (strings) or simple arithmetic
//! (`"bolt_length - head_height"`) anywhere a numeric literal is allowed.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::ir::{CadDocument, Feature, Profile, ThreadKind, ValidationError};

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
///
/// Overall length (`bolt_length`, …) never ratio-scales hex-head `depth` or any
/// value that already matches an independent param (`head_height`, `dead_height`).
/// Length changes shank cylinder height and thread extent only.
pub fn apply_parameter_delta(value: &mut Value, name: &str, old: f64, new: f64) {
    if !old.is_finite() || !new.is_finite() || (old - new).abs() < 1e-12 || old.abs() < 1e-12 {
        return;
    }
    write_parameter_value(value, name, new);
    let protected = protected_parameter_values(value, name);
    let axial_overall = is_axial_overall_name(name);
    let mut replaced_exact = false;
    rewrite_numbers(
        value,
        None,
        old,
        new,
        &mut replaced_exact,
        &protected,
        axial_overall,
    );
    if axial_overall && !replaced_exact {
        bump_shank_axial_fields(value, new - old);
    }
}

fn write_parameter_value(value: &mut Value, name: &str, new: f64) {
    let Some(params) = value.get_mut("parameters").and_then(|p| p.as_object_mut()) else {
        return;
    };
    if !params.contains_key(name) {
        return;
    }
    if let Some(num) = serde_json::Number::from_f64(new) {
        params.insert(name.to_string(), Value::Number(num));
    }
}

fn protected_parameter_values(value: &Value, changing: &str) -> Vec<f64> {
    let Some(params) = value.get("parameters").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    params
        .iter()
        .filter_map(|(k, v)| {
            if k == changing {
                return None;
            }
            if !is_independent_bolt_dim(k) {
                return None;
            }
            v.as_f64()
        })
        .collect()
}

/// Head / dead / wrench size stay put when only overall length changes.
pub fn is_independent_bolt_dim(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(
        n.as_str(),
        "head_height"
            | "hex_height"
            | "head_width"
            | "hex_width"
            | "across_flats"
            | "dead_height"
            | "dead_length"
            | "unthreaded_length"
            | "unthreaded_height"
    ) || n.contains("head_height")
        || n.contains("dead_height")
        || n.contains("dead_length")
        || n.contains("unthreaded")
}

fn rewrite_numbers(
    node: &mut Value,
    key: Option<&str>,
    old: f64,
    new: f64,
    replaced_exact: &mut bool,
    protected: &[f64],
    axial_overall: bool,
) {
    match node {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                rewrite_numbers(
                    v,
                    Some(k.as_str()),
                    old,
                    new,
                    replaced_exact,
                    protected,
                    axial_overall,
                );
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                rewrite_numbers(item, key, old, new, replaced_exact, protected, axial_overall);
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
            // Head / dead height literals must not ride along with bolt_length.
            if protected
                .iter()
                .any(|p| (v - p).abs() <= number_tol(*p).max(0.05))
            {
                return;
            }
            // Hex / fuse extrude depth is the head, not the shank. A ratio
            // match on overall length (depth == L or L/2) is what stretched
            // the M8 hex into a tall prism.
            if axial_overall && key == Some("depth") {
                return;
            }
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
    if is_independent_bolt_dim(&n) {
        return false;
    }
    matches!(
        n.as_str(),
        "length" | "height" | "bolt_length" | "overall_length" | "total_length"
    ) || (n.ends_with("_length") && !n.contains("head") && !n.contains("pitch") && !n.contains("dead"))
}

/// Grow/shrink shank cylinder `height` and thread `length` together.
/// Never bump hex/fuse `depth` — that is head height, not overall length.
fn bump_shank_axial_fields(node: &mut Value, delta: f64) {
    match node {
        Value::Object(map) => {
            let op = map.get("op").and_then(|v| v.as_str()).unwrap_or("");
            let shank_op = matches!(op, "cylinder" | "thread");
            for (k, v) in map.iter_mut() {
                if k == "parameters" {
                    continue;
                }
                if shank_op && matches!(k.as_str(), "height" | "length") {
                    if let Some(n) = v.as_f64() {
                        let nv = (n + delta).max(0.05);
                        if let Some(num) = serde_json::Number::from_f64(nv) {
                            *v = Value::Number(num);
                        }
                    }
                    continue;
                }
                bump_shank_axial_fields(v, delta);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                bump_shank_axial_fields(v, delta);
            }
        }
        _ => {}
    }
}

/// Keep hex-head height and unthreaded dead length at their parameter values.
/// Overall `bolt_length` only changes shank / thread extent.
pub fn bind_independent_bolt_dims(doc: &mut CadDocument) {
    if doc.parameters.is_empty() {
        return;
    }
    let head_h = param_named(
        &doc.parameters,
        &["head_height", "hex_height", "head_depth"],
    );
    let dead_h = param_named(
        &doc.parameters,
        &[
            "dead_height",
            "dead_length",
            "unthreaded_length",
            "unthreaded_height",
        ],
    )
    .unwrap_or(0.0);
    let bolt_l = param_named(
        &doc.parameters,
        &["bolt_length", "overall_length", "total_length"],
    );
    let Some(head_h) = head_h else {
        return;
    };
    for body in &mut doc.bodies {
        if !body_is_hex_bolt(&body.features) {
            continue;
        }
        apply_bolt_envelope(&mut body.features, head_h, dead_h, bolt_l);
    }
}

fn param_named(parameters: &BTreeMap<String, f64>, names: &[&str]) -> Option<f64> {
    for name in names {
        if let Some(&v) = parameters.get(*name) {
            if v.is_finite() && v > 0.0 {
                return Some(v);
            }
        }
    }
    None
}

fn body_is_hex_bolt(features: &[Feature]) -> bool {
    let has_hex = features.iter().any(feature_has_hex);
    let has_thread = features.iter().any(|f| matches!(f, Feature::Thread(_)));
    has_hex && has_thread
}

fn feature_has_hex(feat: &Feature) -> bool {
    match feat {
        Feature::Sketch(op) => matches!(op.profile, Profile::Hex(_)),
        Feature::Fuse(op) => matches!(op.profile, Profile::Hex(_)),
        Feature::Cut(op) => matches!(op.profile, Profile::Hex(_)),
        _ => false,
    }
}

fn hex_before_thread(features: &[Feature]) -> bool {
    let hex_i = features.iter().position(feature_has_hex);
    let thread_i = features.iter().position(|f| matches!(f, Feature::Thread(_)));
    match (hex_i, thread_i) {
        (Some(h), Some(t)) => h < t,
        (Some(_), None) => true,
        _ => false,
    }
}

fn apply_bolt_envelope(
    features: &mut [Feature],
    head_h: f64,
    dead_h: f64,
    bolt_l: Option<f64>,
) {
    let hex_first = hex_before_thread(features);
    let mut pending_hex_extrude = false;
    let mut thread_end_z = 0.0_f64;
    for feat in features.iter_mut() {
        match feat {
            Feature::Sketch(op) if matches!(op.profile, Profile::Hex(_)) => {
                pending_hex_extrude = true;
            }
            Feature::Extrude(op) if pending_hex_extrude => {
                op.depth = head_h;
                pending_hex_extrude = false;
            }
            Feature::Fuse(op) if matches!(op.profile, Profile::Hex(_)) => {
                op.depth = head_h;
                if !hex_first {
                    if let Some(l) = bolt_l {
                        op.at[2] = (l - head_h).max(0.0);
                    }
                }
            }
            Feature::Cylinder(op) if hex_first => {
                if let Some(l) = bolt_l {
                    let overlap = cylinder_head_overlap(op.at[2], op.height, head_h, l);
                    op.height = (l - head_h + overlap).max(0.05);
                    op.at[2] = head_h - overlap;
                }
            }
            Feature::Thread(op) if matches!(op.kind, ThreadKind::External) => {
                if let Some(l) = bolt_l {
                    if hex_first {
                        let start = head_h + dead_h;
                        op.length = (l - start).max(0.05);
                        op.at[2] = start;
                    } else {
                        op.length = (l - head_h - dead_h).max(0.05);
                    }
                    thread_end_z = op.at[2] + op.length;
                }
            }
            _ => {
                if !matches!(feat, Feature::Sketch(_)) {
                    pending_hex_extrude = false;
                }
            }
        }
    }
    let _ = thread_end_z;
}

fn cylinder_head_overlap(at_z: f64, height: f64, head_h: f64, bolt_l: f64) -> f64 {
    let from_at = head_h - at_z;
    if from_at > 0.05 && from_at < 3.0 {
        return from_at;
    }
    let from_len = height - (bolt_l - head_h);
    if from_len > 0.05 && from_len < 3.0 {
        return from_len;
    }
    1.0
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
    use crate::ir::{CadDocument, Feature};
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
    fn delta_length_does_not_scale_hex_extrude_depth() {
        // Agent often bakes hex extrude depth equal to bolt_length (or L/2).
        // Changing length must not stretch that prism.
        let mut doc = json!({
            "parameters": { "bolt_length": 40.0, "head_height": 5.3, "head_width": 13.0 },
            "bodies": [{
                "features": [
                    { "op": "sketch", "profile": { "hex": { "across_flats": 13.0 } } },
                    { "op": "extrude", "depth": 40.0 },
                    { "op": "cylinder", "diameter": 8, "height": 35.7, "at": [0, 0, 4.3] },
                    { "op": "thread", "length": 34.7, "size": "M8" }
                ]
            }]
        });
        apply_parameter_delta(&mut doc, "bolt_length", 40.0, 64.0);
        let depth = doc["bodies"][0]["features"][1]["depth"].as_f64().unwrap();
        assert!(
            (depth - 40.0).abs() < 1e-9 || (depth - 5.3).abs() < 1e-9,
            "hex extrude must not pick up the new length, depth={depth}"
        );
        assert!((depth - 64.0).abs() > 1.0, "head was ratio-scaled to {depth}");
    }

    #[test]
    fn delta_length_protects_head_and_dead_height() {
        let mut doc = json!({
            "parameters": {
                "bolt_length": 40.0,
                "head_height": 5.3,
                "dead_height": 2.0,
                "head_width": 13.0
            },
            "bodies": [{
                "features": [
                    { "op": "fuse", "depth": 5.3,
                      "profile": { "hex": { "across_flats": 13.0 } } },
                    { "op": "thread", "length": 32.7, "size": "M8" }
                ]
            }]
        });
        apply_parameter_delta(&mut doc, "bolt_length", 40.0, 55.0);
        let depth = doc["bodies"][0]["features"][0]["depth"].as_f64().unwrap();
        assert!((depth - 5.3).abs() < 1e-9, "head_height changed: {depth}");
        let params = doc["parameters"].as_object().unwrap();
        assert!((params["head_height"].as_f64().unwrap() - 5.3).abs() < 1e-9);
        assert!((params["dead_height"].as_f64().unwrap() - 2.0).abs() < 1e-9);
        assert!((params["bolt_length"].as_f64().unwrap() - 55.0).abs() < 1e-9);
    }

    #[test]
    fn bind_snaps_stretched_hex_to_head_height() {
        let mut doc = CadDocument::from_json_value(json!({
            "documentId": "m8",
            "units": "mm",
            "parameters": { "bolt_length": 64.0, "head_height": 5.3, "dead_height": 1.5 },
            "bodies": [{
                "bodyId": "b",
                "features": [
                    { "op": "sketch", "plane": "XY",
                      "profile": { "hex": { "across_flats": 13 } } },
                    { "op": "extrude", "depth": 16.0 },
                    { "op": "cylinder", "diameter": 8, "height": 60.0, "at": [0, 0, 4.3] },
                    { "op": "thread", "kind": "external", "size": "M8",
                      "length": 50.0, "at": [0, 0, 16.0] }
                ]
            }]
        }))
        .unwrap();
        bind_independent_bolt_dims(&mut doc);
        match &doc.bodies[0].features[1] {
            Feature::Extrude(op) => assert!(
                (op.depth - 5.3).abs() < 1e-9,
                "head depth={}",
                op.depth
            ),
            other => panic!("expected extrude, {other:?}"),
        }
        match &doc.bodies[0].features[2] {
            Feature::Cylinder(op) => {
                assert!(
                    (op.height - (64.0 - 5.3 + 1.0)).abs() < 0.05,
                    "shank height={}",
                    op.height
                );
                assert!((op.at[2] - 4.3).abs() < 0.05, "shank at.z={}", op.at[2]);
            }
            other => panic!("expected cylinder, {other:?}"),
        }
        match &doc.bodies[0].features[3] {
            Feature::Thread(op) => {
                assert!(
                    (op.length - (64.0 - 5.3 - 1.5)).abs() < 0.05,
                    "thread length={}",
                    op.length
                );
                assert!((op.at[2] - 6.8).abs() < 0.05, "thread at.z={}", op.at[2]);
            }
            other => panic!("expected thread, {other:?}"),
        }
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
