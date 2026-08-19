//! AgentCAD geometry kernel crate.
//!
//! # Quickstart
//! ```no_run
//! use kernel::{Engine, ir::{CadProgram, Units, Feature, SketchOp, SketchPlane, Profile, RectProfile, ExtrudeOp}};
//! use serde_json::json;
//!
//! let json = r#"{
//!     "units": "mm",
//!     "features": [
//!         { "op": "sketch", "plane": "XY", "profile": { "rect": { "w": 40, "h": 20 } } },
//!         { "op": "extrude", "depth": 5 }
//!     ]
//! }"#;
//! let prog: CadProgram = serde_json::from_str(json).unwrap();
//! let output = Engine::default().execute(&prog).unwrap();
//! println!("volume = {:.1} mm³", output.metrics.volume);
//! ```

pub mod engine;
pub mod export;
pub mod ir;

pub use engine::{
    BodyOutput, DocumentOutput, Engine, ExportFormat, KernelError, MeshData, MetricsData,
    ModelOutput,
};
pub use ir::{CadBody, CadDocument, CadProgram, ValidationError};
