//! Copper zones in the PCB editor: drawing the outline, filling, and the zone commands.
//!
//! The fill itself is computed by `buturecad_eda_core::zonefill` and stored in the zone, so the
//! board, DRC, ratsnest and manufacturing all see the same copper. After every edit near a zone
//! the zone is refilled **in the same undo step** (`PcbEditor::auto_fill`); `fill_zones` forces a
//! complete refill and `unfill_zones` clears the fills.

use super::pcb_editor::PcbTool;
use super::*;
use buturecad_eda_core::model::{PcbChange, PcbObject};
use buturecad_eda_core::zonefill::{fill_changes, stale_zones};
use buturecad_geometry::{Polygon, Segment};
use buturecad_pcb::{layer, PadConnection, Zone};
use buturecad_renderer::{Rgba, Shape};

impl Workspace {
    /// A click with the Zone tool: adds an outline vertex, or closes the outline when the first
    /// vertex is clicked again.
    pub(crate) fn zone_click(&mut self, at: Point, out: &mut Outcome) {
        let pts = &self.pcb_ed.pts;
        if pts.len() >= 3 && at.distance(pts[0]) <= self.route_tolerance() {
            self.zone_close(out);
            return;
        }
        if pts.last() != Some(&at) {
            self.pcb_ed.pts.push(at);
        }
        out.redraw = true;
        out.selection_changed = true; // the bar shows that a zone is being drawn
    }

    /// Closes the outline being drawn and creates the zone (with its fill).
    pub(crate) fn zone_close(&mut self, out: &mut Outcome) {
        let pts = std::mem::take(&mut self.pcb_ed.pts);
        out.redraw = true;
        out.selection_changed = true;
        if pts.len() < 3 {
            self.log(MessageLevel::Warn, "A zone needs at least three corners");
            self.pcb_ed.pts = pts;
            return;
        }
        let ok = Polygon::new(pts.clone()).is_ok_and(|p| p.is_simple() && p.area() > 0.0);
        if !ok {
            self.log(
                MessageLevel::Warn,
                "The zone outline crosses itself or has no area; keep drawing or press Esc",
            );
            self.pcb_ed.pts = pts;
            return;
        }
        let layer = self.active_copper();
        let net = self.pcb_ed.zone_net;
        let id = self.pcb_new_id();
        let zone = Zone::new(layer, pts, net);
        if self.pcb_commit(
            "Add zone",
            vec![PcbChange::create(id, PcbObject::Zone(zone))],
        ) {
            self.selection.insert(Mode::Pcb, BTreeSet::from([id]));
            out.model_changed = true;
            let filled = self
                .pcb_doc()
                .and_then(|d| d.zones.get(&id))
                .and_then(|z| z.fill.as_ref())
                .map_or(0, |f| f.regions.len());
            self.log(
                MessageLevel::Info,
                format!("Zone on {} filled: {filled} piece(s)", layer::name(layer)),
            );
        }
    }

    pub(crate) fn zone_drawing(&self) -> bool {
        self.pcb_ed.tool == PcbTool::Zone && !self.pcb_ed.pts.is_empty()
    }

    /// The outline so far and the rubber band to the pointer.
    pub(crate) fn zone_preview_shapes(&self, colour: Rgba) -> Vec<(Shape, Rgba)> {
        let pts = &self.pcb_ed.pts;
        if self.pcb_ed.tool != PcbTool::Zone || pts.is_empty() {
            return Vec::new();
        }
        let mut path = pts.clone();
        path.push(self.cursor);
        let mut out: Vec<(Shape, Rgba)> = path
            .windows(2)
            .map(|w| {
                (
                    Shape::Line {
                        segment: Segment::new(w[0], w[1]),
                        width: 100_000,
                    },
                    colour,
                )
            })
            .collect();
        if pts.len() >= 3 {
            out.push((
                Shape::Line {
                    segment: Segment::new(self.cursor, pts[0]),
                    width: 40_000,
                },
                colour,
            ));
        }
        out
    }

    /// Zone actions from the IPC layer: `fill_zones`, `unfill_zones`, `zone_net` (`<net id>` or
    /// `none`), `auto_fill` (toggle).
    pub(crate) fn pcb_zone_command(
        &mut self,
        name: &str,
        arg: Option<&str>,
    ) -> Result<bool, String> {
        match name {
            "fill_zones" => {
                let doc = self.pcb_doc().ok_or("no project is open")?;
                if doc.zones.is_empty() {
                    return Err("there are no zones on the board".into());
                }
                let changes = fill_changes(doc, None, true);
                if changes.is_empty() {
                    self.log(MessageLevel::Info, "The zone fills are up to date");
                } else {
                    let n = changes.len();
                    if !self.pcb_commit("Fill zones", changes) {
                        return Err("the fill was rejected (see Messages)".into());
                    }
                    self.log(MessageLevel::Info, format!("Filled {n} zone(s)"));
                }
                Ok(true)
            }
            "unfill_zones" => {
                let doc = self.pcb_doc().ok_or("no project is open")?;
                let changes: Vec<PcbChange> = doc
                    .zones
                    .iter()
                    .filter(|(_, z)| z.fill.is_some())
                    .map(|(id, z)| {
                        let mut n = z.clone();
                        n.fill = None;
                        PcbChange::modify(*id, PcbObject::Zone(z.clone()), PcbObject::Zone(n))
                    })
                    .collect();
                if changes.is_empty() {
                    return Ok(true);
                }
                // Clearing must not trigger the automatic refill.
                let keep = std::mem::replace(&mut self.pcb_ed.auto_fill, false);
                let ok = self.pcb_commit("Clear zone fills", changes);
                self.pcb_ed.auto_fill = keep;
                if ok {
                    Ok(true)
                } else {
                    Err("the change was rejected (see Messages)".into())
                }
            }
            "zone_net" => {
                let a = arg.unwrap_or("").trim();
                self.pcb_ed.zone_net = if a.is_empty() || a == "none" {
                    None
                } else {
                    let id = Id(a.parse().map_err(|_| "unknown net")?);
                    if !self.pcb_doc().is_some_and(|d| d.nets.contains_key(&id)) {
                        return Err("unknown net".into());
                    }
                    Some(id)
                };
                Ok(true)
            }
            "auto_fill" => {
                self.pcb_ed.auto_fill = !self.pcb_ed.auto_fill;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Zones that need a refill (for the bar).
    pub(crate) fn stale_zone_count(&self) -> u32 {
        self.pcb_doc().map_or(0, |d| stale_zones(d).len() as u32)
    }

    pub(crate) fn zone_pad_connection_options() -> Vec<(String, String)> {
        [
            PadConnection::Thermal,
            PadConnection::Solid,
            PadConnection::None,
        ]
        .into_iter()
        .map(|c| {
            (
                c.id().to_owned(),
                match c {
                    PadConnection::Thermal => "Thermal relief",
                    PadConnection::Solid => "Solid",
                    PadConnection::None => "None (isolated)",
                }
                .to_owned(),
            )
        })
        .collect()
    }
}
