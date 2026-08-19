//! JSON Intermediate Representation for the AgentCAD feature tree.
//!
//! This is the language the LLM writes. Keeping it small prevents the
//! hallucination problem that kills CadQuery/Build123d (large APIs → model
//! reaches for non-existent methods). Everything is schema-validated by serde
//! before touching the geometry kernel.

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Top-level program ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CadProgram {
    pub units: Units,
    pub features: Vec<Feature>,
}

// ── Multi-body document ───────────────────────────────────────────────────────

/// A design is a set of independent bodies, each with its own feature tree.
/// Legacy `{ units, features }` programs wrap as a single body via [`CadDocument::from_program`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CadDocument {
    #[serde(default = "default_document_id")]
    pub document_id: String,
    pub units: Units,
    pub bodies: Vec<CadBody>,
}

fn default_document_id() -> String {
    "document".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CadBody {
    pub body_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub visible: bool,
    #[serde(default)]
    pub suppressed: bool,
    #[serde(default)]
    pub transform: BodyTransform,
    pub features: Vec<Feature>,
    /// This body is the *tool*. `cut`/`fuse` is applied onto `target`.
    #[serde(default)]
    pub references: Vec<BodyReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BodyTransform {
    #[serde(default)]
    pub position: [f64; 3],
    /// Euler XYZ in degrees.
    #[serde(default)]
    pub rotation: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BodyReference {
    pub op: BodyRefOp,
    pub target: String,
    /// Hide this tool body after the boolean (consumed cutter).
    #[serde(default)]
    pub consume: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BodyRefOp {
    Cut,
    Fuse,
}

impl CadDocument {
    pub fn from_program(program: CadProgram) -> Self {
        CadDocument {
            document_id: default_document_id(),
            units: program.units,
            bodies: vec![CadBody {
                body_id: "body_main".to_string(),
                name: "Body".to_string(),
                visible: true,
                suppressed: false,
                transform: BodyTransform::default(),
                features: program.features,
                references: vec![],
            }],
        }
    }

    /// Accept a document `{ bodies: [...] }` or a legacy program `{ features: [...] }`.
    pub fn from_json_value(value: serde_json::Value) -> Result<Self, String> {
        if value.get("bodies").is_some() {
            serde_json::from_value(value).map_err(|e| e.to_string())
        } else if value.get("features").is_some() {
            let program: CadProgram =
                serde_json::from_value(value).map_err(|e| e.to_string())?;
            Ok(Self::from_program(program))
        } else {
            Err("expected a CadDocument (bodies) or CadProgram (features)".into())
        }
    }

    pub fn as_program(&self) -> CadProgram {
        CadProgram {
            units: self.units.clone(),
            features: self
                .bodies
                .first()
                .map(|b| b.features.clone())
                .unwrap_or_default(),
        }
    }

    pub fn replace_body(&mut self, body: CadBody) {
        if let Some(existing) = self
            .bodies
            .iter_mut()
            .find(|b| b.body_id == body.body_id)
        {
            *existing = body;
        } else {
            self.bodies.push(body);
        }
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.bodies.is_empty() {
            return Err(ValidationError::EmptyFeatures);
        }
        let mut seen = std::collections::HashSet::new();
        for (bi, body) in self.bodies.iter().enumerate() {
            if body.body_id.trim().is_empty() {
                return Err(ValidationError::InvalidParameter {
                    index: bi,
                    message: "bodyId must not be empty".into(),
                });
            }
            if !seen.insert(body.body_id.clone()) {
                return Err(ValidationError::InvalidParameter {
                    index: bi,
                    message: format!("duplicate bodyId '{}'", body.body_id),
                });
            }
            if body.features.is_empty() {
                return Err(ValidationError::InvalidParameter {
                    index: bi,
                    message: format!("body '{}' has no features", body.body_id),
                });
            }
            for (fi, feat) in body.features.iter().enumerate() {
                feat.validate(fi)?;
            }
            for r in &body.references {
                if r.target == body.body_id {
                    return Err(ValidationError::InvalidParameter {
                        index: bi,
                        message: "reference target cannot be this body".into(),
                    });
                }
            }
        }
        Ok(())
    }
}

impl CadBody {
    pub fn display_name(&self) -> &str {
        if self.name.trim().is_empty() {
            &self.body_id
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    Mm,
    #[serde(rename = "in")]
    Inch,
}

// ── Feature enum (the LLM writes one of these per step) ────────────────────

/// Every feature has an `"op"` string tag that serde uses to pick the variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Feature {
    Sketch(SketchOp),
    Extrude(ExtrudeOp),
    Revolve(RevolveOp),
    Cut(CutOp),
    Fuse(FuseOp),
    Hole(HoleOp),
    Fillet(FilletOp),
    Chamfer(ChamferOp),
    Transform(TransformOp),
    Box(BoxOp),
    Cylinder(CylinderOp),
    Sphere(SphereOp),
    Cone(ConeOp),
    Torus(TorusOp),
    Loft(LoftOp),
    Mirror(MirrorOp),
    Pattern(PatternOp),
    Shell(ShellOp),
    DraftExtrude(DraftExtrudeOp),
}

// ── Sketch ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SketchOp {
    /// Optional identifier for referencing later (unused in v0 but in schema).
    #[serde(default = "default_sketch_id")]
    pub id: String,
    /// Which construction plane to sketch on.
    #[serde(default)]
    pub plane: SketchPlane,
    /// The 2-D profile to be sketched.
    pub profile: Profile,
    /// 2-D offset of the profile origin on the chosen plane.
    #[serde(default)]
    pub origin: [f64; 2],
}

fn default_sketch_id() -> String {
    "sketch".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SketchPlane {
    #[default]
    XY,
    XZ,
    YZ,
}

// ── Profile variants ─────────────────────────────────────────────────────────

/// External serde tagging: `{ "rect": { … } }` or `{ "circle": { … } }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Rect(RectProfile),
    Circle(CircleProfile),
    Polyline(PolylineProfile),
    Arc(ArcProfile),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RectProfile {
    /// Width along the plane's first axis.
    pub w: f64,
    /// Height along the plane's second axis.
    pub h: f64,
    /// 2-D position of the rectangle. Interpretation depends on `centered`.
    #[serde(default)]
    pub at: [f64; 2],
    /// When true (the default), `at` is the **center** of the rectangle so a
    /// 120×120 rect at [0, 0] spans [-60, 60] on both axes — not the +X+Y
    /// quadrant. Set `false` to treat `at` as the min-corner (legacy).
    #[serde(default = "default_true")]
    pub centered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CircleProfile {
    /// Diameter.
    pub d: f64,
    /// Center position on the plane (default origin).
    #[serde(default)]
    pub at: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolylineProfile {
    /// At least 3 points in 2-D plane coordinates.
    pub points: Vec<[f64; 2]>,
    /// If true the last point is connected back to the first.
    #[serde(default = "default_true")]
    pub closed: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArcProfile {
    pub center: [f64; 2],
    pub radius: f64,
    /// Start angle in degrees.
    pub start_angle: f64,
    /// End angle in degrees.
    pub end_angle: f64,
}

// ── Extrude ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExtrudeOp {
    #[serde(default = "default_extrude_id")]
    pub id: String,
    /// Positive depth in the sketch plane's normal direction.
    pub depth: f64,
    /// If true, extrude depth/2 in both directions.
    #[serde(default)]
    pub symmetric: bool,
}

fn default_extrude_id() -> String {
    "body".to_string()
}

// ── Revolve ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RevolveOp {
    #[serde(default = "default_revolve_id")]
    pub id: String,
    /// Revolution angle in degrees (0 < angle ≤ 360).
    #[serde(default = "default_revolve_angle")]
    pub angle: f64,
    /// Axis of revolution.
    pub axis: RevolveAxis,
    /// Point on the axis (default origin).
    #[serde(default)]
    pub origin: [f64; 3],
}

fn default_revolve_id() -> String {
    "revolve".to_string()
}
fn default_revolve_angle() -> f64 {
    360.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RevolveAxis {
    X,
    Y,
    Z,
}

// ── Boolean cut / fuse ───────────────────────────────────────────────────────

/// Boolean subtract: creates an extruded tool solid from `profile` at `at`
/// and cuts it from the current solid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CutOp {
    pub profile: Profile,
    /// Extrude depth of the tool solid. For through-cuts this is a minimum;
    /// the kernel extends the tool through the whole solid.
    pub depth: f64,
    /// 3-D position of the tool profile.
    #[serde(default)]
    pub at: [f64; 3],
    /// Plane on which the profile sits.
    #[serde(default)]
    pub plane: SketchPlane,
    /// When true (default), the cutter is extended through the entire solid
    /// so a slightly-too-short or wrong-side `at` still punches through.
    /// Set false for a blind pocket of exactly `depth`.
    #[serde(default = "default_true")]
    pub through: bool,
}

/// Boolean union: adds an extruded solid to the current solid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FuseOp {
    pub profile: Profile,
    pub depth: f64,
    #[serde(default)]
    pub at: [f64; 3],
    #[serde(default)]
    pub plane: SketchPlane,
}

// ── Hole (convenience boolean subtract) ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HoleOp {
    /// Hole diameter.
    pub diameter: f64,
    /// Depth of the hole. Ignored when `through` is true (the default): the
    /// cutter is sized to the solid's bounding box so it always punches through.
    pub depth: f64,
    /// 2-D center position on the hole plane.
    pub center: [f64; 2],
    /// Plane the hole is normal to (default XY → hole goes in Z).
    #[serde(default)]
    pub plane: SketchPlane,
    /// Default true: through-hole. Set false for a blind hole of `depth`.
    #[serde(default = "default_true")]
    pub through: bool,
}

// ── Fillet / chamfer ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FilletOp {
    /// Fillet radius (must be positive).
    pub radius: f64,
    /// `"all"` to fillet every edge, or a list of zero-based edge indices from
    /// `list_topology`.
    #[serde(default)]
    pub edges: EdgeSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChamferOp {
    /// Chamfer distance (must be positive).
    pub distance: f64,
    #[serde(default)]
    pub edges: EdgeSelection,
}

/// Flexible edge selector: JSON string `"all"` or an integer array `[0, 3, 7]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EdgeSelection {
    /// Named selection – only `"all"` is defined in v0.
    Named(String),
    /// Explicit edge indices as returned by `list_topology`.
    Indices(Vec<usize>),
}

impl Default for EdgeSelection {
    fn default() -> Self {
        EdgeSelection::Named("all".to_string())
    }
}

impl EdgeSelection {
    pub fn is_all(&self) -> bool {
        matches!(self, EdgeSelection::Named(s) if s == "all")
    }
}

// ── Transform ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransformOp {
    pub translate: Option<[f64; 3]>,
    pub rotate: Option<RotateParams>,
    pub scale: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RotateParams {
    /// Normalised rotation axis [x, y, z].
    pub axis: [f64; 3],
    /// Rotation angle in degrees.
    pub angle: f64,
    /// Pivot point (default origin).
    #[serde(default)]
    pub origin: [f64; 3],
}

// ── Primitive solids (can start a program; no sketch required) ───────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoxOp {
    /// [dx, dy, dz] — all must be positive.
    pub size: [f64; 3],
    /// Placement. With `centered` (default) this is the XY center; the box
    /// sits on Z = `at[2]` (bottom at at.z).
    #[serde(default)]
    pub at: [f64; 3],
    #[serde(default = "default_true")]
    pub centered: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CylinderOp {
    pub diameter: f64,
    pub height: f64,
    /// Axis origin. Cylinder is always centered on its axis in XY.
    #[serde(default)]
    pub at: [f64; 3],
    #[serde(default = "default_axis_z")]
    pub axis: RevolveAxis,
}

fn default_axis_z() -> RevolveAxis {
    RevolveAxis::Z
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SphereOp {
    pub diameter: f64,
    /// Sphere is 3-D centered on `at`.
    #[serde(default)]
    pub at: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConeOp {
    /// Base diameter (at z = at[2]).
    pub d1: f64,
    /// Top diameter. Use 0 for a pointed cone / circular pyramid.
    pub d2: f64,
    pub height: f64,
    #[serde(default)]
    pub at: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TorusOp {
    pub major: f64,
    pub minor: f64,
    #[serde(default)]
    pub at: [f64; 3],
}

// ── Loft ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoftOp {
    /// Two or more 2-D sections (or one section plus `apex`).
    pub sections: Vec<LoftSection>,
    /// `true` (default) = straight sides (pyramid / frustum). `false` = smooth.
    #[serde(default = "default_true")]
    pub ruled: bool,
    /// Optional point that the loft tapers to (square/circular pyramid tip).
    pub apex: Option<[f64; 3]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoftSection {
    pub profile: Profile,
    /// 3-D placement of this section. For a pyramid, keep XY at 0 and increase Z.
    #[serde(default)]
    pub at: [f64; 3],
}

// ── Mirror / pattern / shell / draft ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MirrorOp {
    /// Mirror plane. `YZ` flips X, `XZ` flips Y, `XY` flips Z.
    pub plane: SketchPlane,
    /// A point on the mirror plane (default origin).
    #[serde(default)]
    pub origin: [f64; 3],
    /// Union the mirrored copy with the original (default true).
    #[serde(default = "default_true")]
    pub fuse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatternOp {
    pub kind: PatternKind,
    /// Total number of instances including the original. Must be ≥ 2.
    pub count: u32,
    /// Linear: world-space spacing between instances.
    pub spacing: Option<f64>,
    /// Linear: direction vector (need not be unit length).
    pub direction: Option<[f64; 3]>,
    /// Circular: axis of rotation.
    pub axis: Option<RevolveAxis>,
    /// Circular: angle in degrees between consecutive instances (default 360/count).
    pub angle: Option<f64>,
    /// Circular: center of the pattern.
    #[serde(default)]
    pub center: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PatternKind {
    Linear,
    Circular,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShellOp {
    /// Wall thickness. Positive = grow outward, negative = hollow inward.
    pub thickness: f64,
    /// Faces to open. `"all"` opens nothing (closed hollow); indices open those faces.
    #[serde(default)]
    pub faces: EdgeSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DraftExtrudeOp {
    /// Extrude depth of the last sketch.
    pub depth: f64,
    /// Draft angle in degrees. Positive tapers inward toward the top.
    pub angle: f64,
}

// ── Validation ────────────────────────────────────────────────────────────────

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("Empty features list")]
    EmptyFeatures,
    #[error("Feature {index}: {message}")]
    InvalidParameter { index: usize, message: String },
}

impl CadProgram {
    /// Validate all features in order. Returns the first error found.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.features.is_empty() {
            return Err(ValidationError::EmptyFeatures);
        }
        for (i, feat) in self.features.iter().enumerate() {
            feat.validate(i)?;
        }
        Ok(())
    }
}

impl Feature {
    pub fn validate(&self, index: usize) -> Result<(), ValidationError> {
        let err = |msg: &str| ValidationError::InvalidParameter {
            index,
            message: msg.to_string(),
        };

        match self {
            Feature::Sketch(op) => match &op.profile {
                Profile::Rect(r) => {
                    if r.w <= 0.0 {
                        return Err(err("rect.w must be positive"));
                    }
                    if r.h <= 0.0 {
                        return Err(err("rect.h must be positive"));
                    }
                }
                Profile::Circle(c) => {
                    if c.d <= 0.0 {
                        return Err(err("circle.d (diameter) must be positive"));
                    }
                }
                Profile::Polyline(p) => {
                    if p.points.len() < 3 {
                        return Err(err("polyline must have at least 3 points"));
                    }
                }
                Profile::Arc(a) => {
                    if a.radius <= 0.0 {
                        return Err(err("arc.radius must be positive"));
                    }
                }
            },
            Feature::Extrude(op) => {
                if op.depth <= 0.0 {
                    return Err(err(&format!(
                        "extrude.depth must be positive (got {})",
                        op.depth
                    )));
                }
            }
            Feature::Revolve(op) => {
                if op.angle <= 0.0 || op.angle > 360.0 {
                    return Err(err(&format!(
                        "revolve.angle must be in (0, 360] (got {})",
                        op.angle
                    )));
                }
            }
            Feature::Cut(op) => {
                if op.depth <= 0.0 {
                    return Err(err(&format!(
                        "cut.depth must be positive (got {})",
                        op.depth
                    )));
                }
            }
            Feature::Fuse(op) => {
                if op.depth <= 0.0 {
                    return Err(err(&format!(
                        "fuse.depth must be positive (got {})",
                        op.depth
                    )));
                }
            }
            Feature::Hole(op) => {
                if op.diameter <= 0.0 {
                    return Err(err(&format!(
                        "hole.diameter must be positive (got {})",
                        op.diameter
                    )));
                }
                if op.depth <= 0.0 {
                    return Err(err(&format!(
                        "hole.depth must be positive (got {})",
                        op.depth
                    )));
                }
            }
            Feature::Fillet(op) => {
                if op.radius <= 0.0 {
                    return Err(err(&format!(
                        "fillet.radius must be positive (got {})",
                        op.radius
                    )));
                }
            }
            Feature::Chamfer(op) => {
                if op.distance <= 0.0 {
                    return Err(err(&format!(
                        "chamfer.distance must be positive (got {})",
                        op.distance
                    )));
                }
            }
            Feature::Transform(op) => {
                if let Some(s) = op.scale {
                    if s <= 0.0 {
                        return Err(err(&format!(
                            "transform.scale must be positive (got {})",
                            s
                        )));
                    }
                }
            }
            Feature::Box(op) => {
                if op.size.iter().any(|&v| v <= 0.0) {
                    return Err(err("box.size [dx, dy, dz] must all be positive"));
                }
            }
            Feature::Cylinder(op) => {
                if op.diameter <= 0.0 {
                    return Err(err("cylinder.diameter must be positive"));
                }
                if op.height <= 0.0 {
                    return Err(err("cylinder.height must be positive"));
                }
            }
            Feature::Sphere(op) => {
                if op.diameter <= 0.0 {
                    return Err(err("sphere.diameter must be positive"));
                }
            }
            Feature::Cone(op) => {
                if op.d1 < 0.0 || op.d2 < 0.0 {
                    return Err(err("cone diameters must be >= 0"));
                }
                if op.d1 <= 0.0 && op.d2 <= 0.0 {
                    return Err(err("cone needs at least one positive diameter"));
                }
                if op.height <= 0.0 {
                    return Err(err("cone.height must be positive"));
                }
            }
            Feature::Torus(op) => {
                if op.major <= 0.0 || op.minor <= 0.0 {
                    return Err(err("torus.major and torus.minor must be positive"));
                }
            }
            Feature::Loft(op) => {
                let n = op.sections.len() + usize::from(op.apex.is_some());
                if n < 2 {
                    return Err(err(
                        "loft needs at least two sections, or one section plus apex",
                    ));
                }
                for (si, sec) in op.sections.iter().enumerate() {
                    match &sec.profile {
                        Profile::Rect(r) if r.w <= 0.0 || r.h <= 0.0 => {
                            return Err(err(&format!("loft section {si}: rect size must be positive")));
                        }
                        Profile::Circle(c) if c.d <= 0.0 => {
                            return Err(err(&format!("loft section {si}: circle.d must be positive")));
                        }
                        Profile::Polyline(p) if p.points.len() < 3 => {
                            return Err(err(&format!("loft section {si}: polyline needs ≥ 3 points")));
                        }
                        Profile::Arc(a) if a.radius <= 0.0 => {
                            return Err(err(&format!("loft section {si}: arc.radius must be positive")));
                        }
                        _ => {}
                    }
                }
            }
            Feature::Mirror(_) => {}
            Feature::Pattern(op) => {
                if op.count < 2 {
                    return Err(err("pattern.count must be ≥ 2 (includes the original)"));
                }
                match op.kind {
                    PatternKind::Linear => {
                        if op.spacing.unwrap_or(0.0) <= 0.0 {
                            return Err(err("linear pattern requires positive spacing"));
                        }
                    }
                    PatternKind::Circular => {}
                }
            }
            Feature::Shell(op) => {
                if op.thickness == 0.0 {
                    return Err(err("shell.thickness must be non-zero"));
                }
            }
            Feature::DraftExtrude(op) => {
                if op.depth <= 0.0 {
                    return Err(err("draft_extrude.depth must be positive"));
                }
            }
        }
        Ok(())
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_bracket_json() {
        let json = r#"{
            "units": "mm",
            "features": [
                { "op": "sketch", "plane": "XY", "profile": { "rect": { "w": 40.0, "h": 20.0 } } },
                { "op": "extrude", "depth": 5.0 },
                { "op": "hole",    "diameter": 6.0, "depth": 7.0, "center": [10.0, 10.0] },
                { "op": "fillet",  "edges": "all",  "radius": 1.0 }
            ]
        }"#;
        let prog: CadProgram = serde_json::from_str(json).unwrap();
        assert_eq!(prog.features.len(), 4);
        assert_eq!(prog.units, Units::Mm);
        // Re-serialise and re-parse to verify round-trip
        let again: CadProgram = serde_json::from_str(&serde_json::to_string(&prog).unwrap()).unwrap();
        assert_eq!(prog, again);
    }

    #[test]
    fn rejects_negative_extrude() {
        let prog = CadProgram {
            units: Units::Mm,
            features: vec![
                Feature::Sketch(SketchOp {
                    id: "s".into(),
                    plane: SketchPlane::XY,
                    profile: Profile::Rect(RectProfile { w: 10.0, h: 10.0, at: [0.0; 2], centered: true }),
                    origin: [0.0; 2],
                }),
                Feature::Extrude(ExtrudeOp { id: "b".into(), depth: -1.0, symmetric: false }),
            ],
        };
        assert!(prog.validate().is_err());
    }

    #[test]
    fn rejects_zero_diameter_hole() {
        let prog = CadProgram {
            units: Units::Mm,
            features: vec![
                Feature::Hole(HoleOp {
                    diameter: 0.0,
                    depth: 5.0,
                    center: [0.0; 2],
                    plane: SketchPlane::XY,
                    through: true,
                }),
            ],
        };
        assert!(prog.validate().is_err());
    }

    #[test]
    fn edge_selection_deserialises_all() {
        let fillet: FilletOp = serde_json::from_str(r#"{"radius":1.0,"edges":"all"}"#).unwrap();
        assert!(fillet.edges.is_all());
    }

    #[test]
    fn edge_selection_deserialises_indices() {
        let fillet: FilletOp = serde_json::from_str(r#"{"radius":1.0,"edges":[0,3,5]}"#).unwrap();
        assert!(!fillet.edges.is_all());
    }

    #[test]
    fn default_edge_selection_is_all() {
        let fillet: FilletOp = serde_json::from_str(r#"{"radius":1.0}"#).unwrap();
        assert!(fillet.edges.is_all());
    }

    #[test]
    fn rect_defaults_to_centered() {
        let r: RectProfile = serde_json::from_str(r#"{"w":120.0,"h":120.0}"#).unwrap();
        assert!(r.centered);
        assert_eq!(r.at, [0.0, 0.0]);
    }

    #[test]
    fn box_and_loft_ops_parse() {
        let json = r#"{
            "units": "mm",
            "features": [
                { "op": "box", "size": [120, 120, 10], "centered": true },
                { "op": "loft", "ruled": true,
                  "sections": [
                    { "profile": { "rect": { "w": 100, "h": 100 } }, "at": [0, 0, 0] },
                    { "profile": { "rect": { "w": 20, "h": 20 } }, "at": [0, 0, 50] }
                  ]
                },
                { "op": "mirror", "plane": "YZ" },
                { "op": "pattern", "kind": "linear", "count": 3, "spacing": 20, "direction": [1, 0, 0] }
            ]
        }"#;
        let prog: CadProgram = serde_json::from_str(json).unwrap();
        assert_eq!(prog.features.len(), 4);
        assert!(matches!(prog.features[0], Feature::Box(_)));
        assert!(matches!(prog.features[1], Feature::Loft(_)));
        assert!(matches!(prog.features[2], Feature::Mirror(_)));
        assert!(matches!(prog.features[3], Feature::Pattern(_)));
    }

    #[test]
    fn document_camel_case_round_trip() {
        let json = r#"{
            "documentId": "assembly_001",
            "units": "mm",
            "bodies": [
                {
                    "bodyId": "body_plate",
                    "name": "Plate",
                    "visible": true,
                    "transform": { "position": [0, 0, 0], "rotation": [0, 0, 0] },
                    "features": [
                        { "op": "box", "size": [80, 40, 8], "centered": true }
                    ]
                }
            ]
        }"#;
        let doc: CadDocument = serde_json::from_str(json).unwrap();
        assert_eq!(doc.document_id, "assembly_001");
        assert_eq!(doc.bodies[0].body_id, "body_plate");
        assert!(doc.validate().is_ok());
        let wrapped = CadDocument::from_json_value(serde_json::json!({
            "units": "mm",
            "features": [
                { "op": "box", "size": [10, 10, 10], "centered": true }
            ]
        }))
        .unwrap();
        assert_eq!(wrapped.bodies[0].body_id, "body_main");
    }

    #[test]
    fn replace_body_patches_only_matching_id() {
        let mut doc = CadDocument::from_json_value(serde_json::json!({
            "documentId": "assembly",
            "units": "mm",
            "bodies": [
                { "bodyId": "body_a", "name": "A", "features": [{ "op": "box", "size": [10, 10, 10] }] },
                { "bodyId": "body_b", "name": "B", "features": [{ "op": "box", "size": [4, 4, 4] }] }
            ]
        }))
        .unwrap();
        doc.replace_body(CadBody {
            body_id: "body_b".into(),
            name: "Bracket".into(),
            visible: true,
            suppressed: false,
            transform: BodyTransform::default(),
            features: vec![Feature::Box(BoxOp {
                size: [8.0, 8.0, 8.0],
                at: [0.0; 3],
                centered: true,
            })],
            references: vec![],
        });
        assert_eq!(doc.bodies.len(), 2);
        assert_eq!(doc.bodies[1].name, "Bracket");
        match &doc.bodies[1].features[0] {
            Feature::Box(b) => assert_eq!(b.size, [8.0, 8.0, 8.0]),
            other => panic!("expected box, got {other:?}"),
        }
    }
}
