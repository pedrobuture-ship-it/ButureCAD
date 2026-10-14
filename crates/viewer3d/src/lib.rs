//! 3D PCB visualisation, **decoupled from the PCB core**.
//!
//! The viewer knows nothing about boards, nets or the Command Engine: it receives a plain
//! [`Model`] (millimetres, simple polygons and segments — built from the board by the desktop
//! app), turns it into a triangle [`mesh::Mesh`] and draws it with a small deterministic software
//! rasteriser ([`raster`]): perspective or orthographic [`camera::Camera`], z-buffer, flat
//! shading. No GPU, window or network is involved, so the viewer is testable headless.
//!
//! What is drawn: the substrate (outline extruded to the stack thickness, drilled where vias and
//! through pads are), solder mask over both sides (following the copper), copper tracks, pads,
//! vias and zone fills, silkscreen, and component bodies as boxes. Component *3D models* are not
//! parsed yet (STEP arrives with roadmap stage 28): bodies are boxes from the footprint
//! courtyard, tinted when the footprint references a model.

pub mod camera;
pub mod mesh;
pub mod model;
pub mod raster;

pub use camera::{Camera, Projection};
pub use mesh::{build_mesh, Mesh, Triangle};
pub use model::{Body, Model, Pad, Region, Segment, ViaHole, Visibility};
pub use raster::render;
