//! ISO metric and unified-inch thread designations.
//!
//! Diameters and pitches are stored in millimetres. Convert to document
//! units at the call site (`mm` as-is, `in` ÷ 25.4).

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadSpec {
    pub designation: String,
    /// Major diameter in millimetres.
    pub major_diameter: f64,
    /// Pitch in millimetres.
    pub pitch: f64,
}

/// ISO 261 coarse pitches (mm).
const METRIC_COARSE: &[(f64, f64)] = &[
    (1.0, 0.25),
    (1.2, 0.25),
    (1.4, 0.3),
    (1.6, 0.35),
    (1.8, 0.35),
    (2.0, 0.4),
    (2.2, 0.45),
    (2.5, 0.45),
    (3.0, 0.5),
    (3.5, 0.6),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (7.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (33.0, 3.5),
    (36.0, 4.0),
    (39.0, 4.0),
    (42.0, 4.5),
    (45.0, 4.5),
    (48.0, 5.0),
    (52.0, 5.0),
    (56.0, 5.5),
    (60.0, 5.5),
    (64.0, 6.0),
    (68.0, 6.0),
    (72.0, 6.0),
    (80.0, 6.0),
    (90.0, 6.0),
    (100.0, 6.0),
];

/// Parse a designation such as `M8`, `M8x1`, `1/4-20`, or `#8-32`.
/// Result is always millimetres.
pub fn parse_size(raw: &str) -> Result<ThreadSpec, String> {
    let mut s = raw.trim().to_ascii_uppercase();
    s = s.replace('×', "X");
    s = s.replace('"', "");
    s.retain(|c| !c.is_whitespace());
    for prefix in ["UNC", "UNF", "UNS", "UN"] {
        if s.starts_with(prefix) && s.len() > prefix.len() {
            let rest = &s[prefix.len()..];
            if rest.starts_with('-') || rest.starts_with('X') || rest.starts_with('#') || rest.starts_with(char::is_numeric) {
                s = rest.trim_start_matches('-').to_string();
            }
            break;
        }
    }
    if s.ends_with("UNC") || s.ends_with("UNF") {
        s.truncate(s.len() - 3);
        s = s.trim_end_matches('-').to_string();
    }

    if let Some(rest) = s.strip_prefix('M') {
        return parse_metric(rest);
    }
    parse_inch(&s)
}

fn parse_metric(rest: &str) -> Result<ThreadSpec, String> {
    let rest = rest.replace(',', ".");
    let (d_str, p_str) = if let Some(i) = rest.find('X') {
        (&rest[..i], Some(&rest[i + 1..]))
    } else {
        (rest.as_str(), None)
    };
    let d: f64 = d_str.parse().map_err(|_| {
        format!("could not parse metric size 'M{rest}' (try M8 or M8x1)")
    })?;
    if d <= 0.0 {
        return Err("metric thread diameter must be positive".into());
    }
    let pitch = if let Some(p) = p_str {
        let p: f64 = p.parse().map_err(|_| format!("could not parse pitch in 'M{rest}'"))?;
        if p <= 0.0 {
            return Err("thread pitch must be positive".into());
        }
        p
    } else {
        metric_coarse_pitch(d).ok_or_else(|| {
            format!("no ISO coarse pitch for M{d}; give pitch explicitly (e.g. M{d}x1)")
        })?
    };
    Ok(ThreadSpec {
        designation: if p_str.is_some() {
            format!("M{d}x{pitch}")
        } else {
            format!("M{d}")
        },
        major_diameter: d,
        pitch,
    })
}

fn metric_coarse_pitch(d: f64) -> Option<f64> {
    METRIC_COARSE
        .iter()
        .find(|(dia, _)| (*dia - d).abs() < 1e-6)
        .map(|(_, p)| *p)
        .or_else(|| {
            METRIC_COARSE
                .iter()
                .min_by(|a, b| {
                    (a.0 - d)
                        .abs()
                        .partial_cmp(&(b.0 - d).abs())
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .filter(|(dia, _)| (dia - d).abs() / d.max(1.0) < 0.05)
                .map(|(_, p)| *p)
        })
}

fn parse_inch(s: &str) -> Result<ThreadSpec, String> {
    let s = s.trim_start_matches('#');
    let (dia_str, tpi_str) = s
        .rsplit_once('-')
        .or_else(|| s.rsplit_once('X'))
        .ok_or_else(|| {
            format!("could not parse thread size '{s}' (try M8, M8x1, 1/4-20, or #8-32)")
        })?;
    let tpi: f64 = tpi_str.parse().map_err(|_| {
        format!("could not parse threads-per-inch in '{s}'")
    })?;
    if tpi <= 0.0 {
        return Err("threads-per-inch must be positive".into());
    }
    let dia_in = parse_inch_diameter(dia_str)?;
    if dia_in <= 0.0 {
        return Err("inch thread diameter must be positive".into());
    }
    Ok(ThreadSpec {
        designation: format!("{dia_str}-{tpi_str}"),
        major_diameter: dia_in * 25.4,
        pitch: 25.4 / tpi,
    })
}

/// Numbered machine screws use D = 0.060 + N×0.013 inches (#0–#12).
fn parse_inch_diameter(s: &str) -> Result<f64, String> {
    if let Some(i) = s.find('/') {
        let n: f64 = s[..i]
            .parse()
            .map_err(|_| format!("bad fraction '{s}'"))?;
        let d: f64 = s[i + 1..]
            .parse()
            .map_err(|_| format!("bad fraction '{s}'"))?;
        if d == 0.0 {
            return Err("fraction denominator cannot be 0".into());
        }
        return Ok(n / d);
    }
    let n: f64 = s.parse().map_err(|_| format!("bad diameter '{s}'"))?;
    if (0.0..=12.0).contains(&n) && (n - n.round()).abs() < 1e-9 {
        Ok(0.060 + n * 0.013)
    } else {
        Ok(n)
    }
}

/// ISO 68-1 fundamental triangle height H = (√3/2)·P.
pub fn triangle_height(pitch: f64) -> f64 {
    0.866_025_403_784_438_6 * pitch
}

/// External thread depth of engagement (5H/8), millimetres.
pub fn external_depth(pitch: f64) -> f64 {
    0.625 * triangle_height(pitch)
}

/// Typical 75% tap-drill diameter ≈ major − pitch (shop rule for coarse metric).
pub fn tap_drill_diameter(major: f64, pitch: f64) -> f64 {
    (major - pitch).max(major * 0.5)
}

/// Convert a millimetre spec into document units.
pub fn to_units(spec: &ThreadSpec, inch: bool) -> ThreadSpec {
    if !inch {
        return spec.clone();
    }
    ThreadSpec {
        designation: spec.designation.clone(),
        major_diameter: spec.major_diameter / 25.4,
        pitch: spec.pitch / 25.4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m8_coarse() {
        let s = parse_size("M8").unwrap();
        assert!((s.major_diameter - 8.0).abs() < 1e-9);
        assert!((s.pitch - 1.25).abs() < 1e-9);
    }

    #[test]
    fn m8_fine() {
        let s = parse_size("M8x1").unwrap();
        assert!((s.pitch - 1.0).abs() < 1e-9);
    }

    #[test]
    fn quarter_twenty() {
        let s = parse_size("1/4-20").unwrap();
        assert!((s.major_diameter - 6.35).abs() < 1e-6);
        assert!((s.pitch - 1.27).abs() < 0.01);
    }

    #[test]
    fn numbered_eight_thirty_two() {
        let s = parse_size("#8-32").unwrap();
        assert!((s.major_diameter - 0.164 * 25.4).abs() < 1e-6);
    }
}
