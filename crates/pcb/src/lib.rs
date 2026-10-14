//! PCB model: footprints, board objects, layers.
//!
//! **Scope today (roadmap stage 12):** the *footprint* model ([`footprint`]), its editing
//! operations ([`footprint_edit`]) and the parametric generators ([`generators`]). The board
//! itself (outline, placed footprints, nets, tracks, zones) arrives with the PCB core stage
//! (roadmap 13); until then tracks and vias live in `buturecad-eda-core`'s skeleton model.
//!
//! All coordinates are integer nanometres (+Y up) like the rest of the core. Footprints are
//! defined for the **top** side; flipping to the bottom happens at placement.

pub mod board;
pub mod footprint;
pub mod footprint_edit;
pub mod generators;
pub mod layer;
pub mod physical;

pub use board::*;
pub use footprint::*;
pub use layer::{LayerId, Stackup};
pub use physical::{CopperRole, CopperSpec, Dielectric, DielectricKind, PhysicalStack};
