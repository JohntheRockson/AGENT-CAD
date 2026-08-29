//! Document unit semantics for AgentCAD.
//!
//! All IR linear dimensions are expressed in the document's [`Units`]. The
//! geometry kernel (OCCT) is unitless; these types define how numbers in JSON
//! map to physical length, area, and volume for display and verification.

use crate::ir::Units;
use serde::{Deserialize, Serialize};

/// Millimetres per inch (exact conversion factor).
pub const MM_PER_INCH: f64 = 25.4;

/// Runtime view of a document's unit system.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct UnitContext {
    pub units: Units,
}

impl UnitContext {
    pub fn new(units: Units) -> Self {
        Self { units }
    }

    pub fn mm() -> Self {
        Self::new(Units::Mm)
    }

    /// Default absolute tolerance for comparing lengths in document units.
    pub fn linear_tolerance(&self) -> f64 {
        match self.units {
            Units::Mm => 0.5,
            Units::Inch => 0.02,
        }
    }

    /// Relative tolerance (5%) for larger dimensions, combined with [`linear_tolerance`].
    pub fn tolerant_eq(&self, expected: f64, actual: f64) -> bool {
        let abs_tol = self.linear_tolerance();
        let scale = expected.abs().max(actual.abs());
        let rel_tol = if scale >= 10.0 { scale * 0.05 } else { 0.0 };
        (expected - actual).abs() <= abs_tol.max(rel_tol)
    }

    /// Convert a length from document units → millimetres (canonical SI export).
    pub fn linear_to_mm(&self, value: f64) -> f64 {
        self.units.linear_to_mm(value)
    }

    /// Convert millimetres → document units.
    pub fn linear_from_mm(&self, value: f64) -> f64 {
        self.units.linear_from_mm(value)
    }
}

impl Units {
    pub fn linear_to_mm(self, value: f64) -> f64 {
        match self {
            Units::Mm => value,
            Units::Inch => value * MM_PER_INCH,
        }
    }

    pub fn linear_from_mm(self, value: f64) -> f64 {
        match self {
            Units::Mm => value,
            Units::Inch => value / MM_PER_INCH,
        }
    }

    pub fn area_to_mm2(self, value: f64) -> f64 {
        let s = self.linear_to_mm(1.0);
        value * s * s
    }

    pub fn volume_to_mm3(self, value: f64) -> f64 {
        let s = self.linear_to_mm(1.0);
        value * s * s * s
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Inch => "in",
        }
    }

    pub fn length_suffix(self) -> &'static str {
        match self {
            Units::Mm => "mm",
            Units::Inch => "in",
        }
    }

    pub fn area_suffix(self) -> &'static str {
        match self {
            Units::Mm => "mm²",
            Units::Inch => "in²",
        }
    }

    pub fn volume_suffix(self) -> &'static str {
        match self {
            Units::Mm => "mm³",
            Units::Inch => "in³",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inch_linear_conversion() {
        let ctx = UnitContext::new(Units::Inch);
        assert!((ctx.linear_to_mm(1.0) - 25.4).abs() < 1e-9);
        assert!((ctx.linear_from_mm(25.4) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn tolerant_eq_uses_unit_absolute_floor() {
        let inch = UnitContext::new(Units::Inch);
        assert!(inch.tolerant_eq(1.0, 1.015));
        assert!(!inch.tolerant_eq(1.0, 1.05));
    }
}
