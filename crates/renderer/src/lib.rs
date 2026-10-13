//! 2D renderer for ButureCAD.
//!
//! Layering (see `docs/ARCHITECTURE.md`):
//!
//! * `scene`, `view`, `grid`, `snap`, `picking` — pure CPU model and interaction logic, fully
//!   testable without a GPU. The canvas is **not** built from DOM/SVG elements.
//! * `tessellate` — turns the scene into triangle meshes.
//! * `gpu` — wgpu backend rendering to an offscreen texture and reading pixels back.

pub mod font;
pub mod grid;
pub mod picking;
pub mod scene;
pub mod snap;
pub mod view;

pub use grid::{adaptive_spacing, snap_to_grid, GridSettings, GridStyle};
pub use picking::{box_select, pick, pick_all, BoxMode, Selection};
pub use scene::{Item, Layer, LayerId, ObjectId, Rgba, Scene, Shape};
pub use snap::{snap, SnapKind, SnapResult, SnapSettings};
pub use view::View;
pub mod gpu;
pub mod tessellate;
pub use gpu::{Frame, FrameRequest, GpuRenderer, RenderError, RenderStats, Theme};
