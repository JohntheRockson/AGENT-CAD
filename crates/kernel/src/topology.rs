//! Topology queries for the agent (faces/edges with semantic filters).

use serde::{Deserialize, Serialize};

use crate::engine::{Engine, KernelError};
use crate::ir::CadProgram;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyReport {
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub summary: TopologySummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySummary {
    pub face_count: usize,
    pub edge_count: usize,
    pub largest_face: Option<usize>,
    pub top_face: Option<usize>,
    pub bottom_face: Option<usize>,
    pub longest_edge: Option<usize>,
    pub tip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceInfo {
    pub index: usize,
    pub area: f64,
    pub center: [f64; 3],
    pub normal: [f64; 3],
    pub surface_type: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    pub index: usize,
    pub length: f64,
    pub mid: [f64; 3],
    pub curve_type: String,
    pub tags: Vec<String>,
}

impl Engine {
    /// Execute the program and return a rich face/edge listing for agent selection.
    pub fn list_topology(&self, program: &CadProgram) -> Result<TopologyReport, KernelError> {
        program.validate()?;
        if self.uses_occt() {
            #[cfg(feature = "occt")]
            {
                return crate::engine::occt_backend::list_topology_with_occt(program);
            }
        }
        Ok(TopologyReport {
            faces: vec![],
            edges: vec![],
            summary: TopologySummary {
                face_count: 0,
                edge_count: 0,
                largest_face: None,
                top_face: None,
                bottom_face: None,
                longest_edge: None,
                tip: "Topology listing requires the occt feature.".into(),
            },
        })
    }
}
