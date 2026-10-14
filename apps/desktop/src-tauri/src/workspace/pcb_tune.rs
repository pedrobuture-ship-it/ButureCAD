//! Length tuning in the PCB editor.
//!
//! A net can have a **length target** (target ± tolerance, stored in the board rules and set from
//! the bar); a pair leg without one aims at its partner's length within the pair's maximum
//! skew. The *Tune length* tool (`T`) shows, while the pointer is over a track, the serpentine or
//! accordion that would bring the net to its target — with the live numbers *current → predicted*
//! against the target — and a click replaces that track by the meander (one undo step). The plan
//! is validated with the DRC rules on both sides of the track; if neither side is free the bar
//! says why and nothing can be placed.

use super::pcb_editor::{mm_str, parse_mm, PcbTool};
use super::*;
use crate::pcb_scene as ps;
use buturecad_eda_core::commands::SetRulesCommand;
use buturecad_eda_core::length::{length_status, net_length};
use buturecad_eda_core::model::{PcbChange, PcbObject, Track};
use buturecad_geometry::{Coord, Segment};
use buturecad_pcb::{layer, LayerId, LengthTarget};
use buturecad_renderer::{Rgba, Shape};
use buturecad_router::{plan_meander, validate_replacement, TuneError, TuneParams, TuneStyle};

/// User settings of the tuner.
#[derive(Clone, Copy)]
pub(crate) struct TuneSettings {
    pub style: TuneStyle,
    pub amplitude: Coord,
    pub pitch: Coord,
}

impl Default for TuneSettings {
    fn default() -> Self {
        Self {
            style: TuneStyle::Serpentine,
            amplitude: 3_000_000,
            pitch: 800_000,
        }
    }
}

/// What the pointer is over and what tuning it would do.
#[derive(Clone)]
pub(crate) struct TunePreview {
    key: (Point, u64, i64, i64, &'static str),
    track: Id,
    net: Option<Id>,
    layer: LayerId,
    width: Coord,
    /// The meander to place, when one is valid.
    plan: Option<(Vec<Point>, f64)>,
    wanted: f64,
    partial: bool,
    problem: Option<String>,
}

impl Workspace {
    /// The track under `raw` (nearest within the pick tolerance).
    fn tune_track_at(&self, raw: Point) -> Option<(Id, Track)> {
        let doc = self.pcb_doc()?;
        let tol = self.route_tolerance();
        doc.tracks
            .iter()
            .filter(|(_, t)| t.segment.distance_to_point(raw) <= t.width as f64 / 2.0 + tol)
            .min_by(|a, b| {
                a.1.segment
                    .distance_to_point(raw)
                    .partial_cmp(&b.1.segment.distance_to_point(raw))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(id, t)| (*id, t.clone()))
    }

    fn tune_plan(&self, id: Id, t: &Track, raw: Point) -> TunePreview {
        let key = self.tune_key(raw);
        let mut p = TunePreview {
            key,
            track: id,
            net: t.net,
            layer: t.layer,
            width: t.width,
            plan: None,
            wanted: 0.0,
            partial: false,
            problem: None,
        };
        let Some(doc) = self.pcb_doc() else {
            return p;
        };
        let Some(net) = t.net else {
            p.problem = Some("this track has no net".into());
            return p;
        };
        let Some(status) = length_status(doc, net) else {
            p.problem =
                Some("no length target: set one in the bar, or put the net in a pair".into());
            return p;
        };
        p.wanted = -status.difference();
        if p.wanted <= status.tolerance.min(1000.0) && status.within() {
            p.problem = Some("already within the tolerance".into());
            return p;
        }
        if p.wanted <= 0.0 {
            p.problem = Some(format!(
                "the net is {:.3} mm too long; shorten it by rerouting",
                -p.wanted / 1e6
            ));
            return p;
        }
        let s = self.pcb_ed.tune;
        let params = TuneParams {
            style: s.style,
            amplitude: s.amplitude,
            pitch: s.pitch,
            width: t.width,
            clearance: doc.rules.class_of(Some(net)).clearance,
        };
        let mut first_problem: Option<String> = None;
        for left in [true, false] {
            match plan_meander(t.segment, raw, left, p.wanted, &params) {
                Err(TuneError::NothingToAdd) => {
                    p.problem = Some("already long enough".into());
                    return p;
                }
                Err(e) => {
                    first_problem.get_or_insert(e.to_string());
                }
                Ok(plan) => {
                    let bad = validate_replacement(doc, id, t.layer, t.net, t.width, &plan.path);
                    if let Some(v) = bad.first() {
                        first_problem.get_or_insert(format!(
                            "the meander would break a rule: {}",
                            v.message
                        ));
                    } else {
                        p.partial = plan.is_partial();
                        p.plan = Some((plan.path, plan.added));
                        return p;
                    }
                }
            }
        }
        p.problem = first_problem;
        p
    }

    fn tune_key(&self, raw: Point) -> (Point, u64, i64, i64, &'static str) {
        let s = self.pcb_ed.tune;
        (
            raw,
            self.open.as_ref().map_or(0, |o| o.engine.revision()),
            s.amplitude,
            s.pitch,
            s.style.id(),
        )
    }

    /// Recomputes the tuning preview for the pointer position (cached per change) and hides the
    /// track it would replace. Called wherever the pointer or the board changes.
    pub(crate) fn tune_refresh(&mut self) {
        if self.mode != Mode::Pcb || self.pcb_ed.tool != PcbTool::Tune {
            if self.pcb_ed.tune_preview.take().is_some() {
                self.pcb_restore_scene();
            }
            return;
        }
        let raw = self.raw_cursor();
        let key = self.tune_key(raw);
        if self
            .pcb_ed
            .tune_preview
            .as_ref()
            .is_some_and(|p| p.key == key)
        {
            return;
        }
        let new = self
            .tune_track_at(raw)
            .map(|(id, t)| self.tune_plan(id, &t, raw));
        let hide = new.as_ref().filter(|p| p.plan.is_some()).map(|p| p.track);
        let previous: Vec<Id> = self.pcb_ed.touched.iter().copied().collect();
        if let Some(open) = self.open.as_ref() {
            let doc = open.engine.project().pcb();
            for id in previous {
                if Some(id) != hide {
                    let o = doc.object(id);
                    self.pcb_scene.sync(doc, id, o.as_ref());
                    self.pcb_ed.touched.remove(&id);
                }
            }
            if let Some(id) = hide {
                self.pcb_scene.sync(doc, id, None);
                self.pcb_ed.touched.insert(id);
            }
        }
        self.pcb_ed.tune_preview = new.or(Some(TunePreview {
            key,
            track: Id(0),
            net: None,
            layer: layer::F_CU,
            width: 0,
            plan: None,
            wanted: 0.0,
            partial: false,
            problem: None,
        }));
    }

    /// Click with the Tune tool: replaces the track under the pointer by the previewed meander.
    pub(crate) fn tune_click(&mut self, out: &mut Outcome) {
        self.tune_refresh();
        let Some(p) = self.pcb_ed.tune_preview.clone() else {
            return;
        };
        let Some((path, _)) = p.plan.clone() else {
            let why = p
                .problem
                .unwrap_or_else(|| "point at a track of a net with a length target".into());
            self.log(MessageLevel::Warn, format!("Cannot tune: {why}"));
            return;
        };
        self.tune_commit(p.track, p.layer, p.net, p.width, &path, out);
    }

    fn tune_commit(
        &mut self,
        track: Id,
        layer: LayerId,
        net: Option<Id>,
        width: Coord,
        path: &[Point],
        out: &mut Outcome,
    ) {
        let Some(old) = self.pcb_doc().and_then(|d| d.tracks.get(&track).cloned()) else {
            return;
        };
        let mut changes = vec![PcbChange::delete(track, PcbObject::Track(old))];
        let mut created = BTreeSet::new();
        for w in path.windows(2) {
            let id = self.pcb_new_id();
            created.insert(id);
            changes.push(PcbChange::create(
                id,
                PcbObject::Track(Track {
                    layer,
                    segment: Segment::new(w[0], w[1]),
                    width,
                    net,
                }),
            ));
        }
        if self.pcb_commit("Tune length", changes) {
            self.selection.insert(Mode::Pcb, created);
            out.model_changed = true;
            out.selection_changed = true;
        }
        self.pcb_ed.tune_preview = None;
        out.redraw = true;
    }

    /// Tunes the single selected track at its middle (keyboard-friendly alternative to the tool).
    pub(crate) fn tune_selected(&mut self) -> Result<(), String> {
        let ids: Vec<Id> = self
            .selection
            .get(&Mode::Pcb)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        let [id] = ids.as_slice() else {
            return Err("select exactly one track to tune".into());
        };
        let t = self
            .pcb_doc()
            .and_then(|d| d.tracks.get(id).cloned())
            .ok_or("select a track to tune")?;
        let mid = Point::new(
            (t.segment.a.x + t.segment.b.x) / 2,
            (t.segment.a.y + t.segment.b.y) / 2,
        );
        let p = self.tune_plan(*id, &t, mid);
        let (path, _) = p
            .plan
            .ok_or_else(|| p.problem.unwrap_or_else(|| "nothing to tune".into()))?;
        let mut out = Outcome::default();
        self.tune_commit(*id, t.layer, t.net, t.width, &path, &mut out);
        Ok(())
    }

    /// The net whose length the bar shows: the one under the tuner, else the selected tracks'.
    fn length_net(&self) -> Option<Id> {
        if let Some(p) = &self.pcb_ed.tune_preview {
            if p.net.is_some() {
                return p.net;
            }
        }
        let doc = self.pcb_doc()?;
        let nets: BTreeSet<Option<Id>> = self
            .selection
            .get(&Mode::Pcb)?
            .iter()
            .filter_map(|id| doc.tracks.get(id))
            .map(|t| t.net)
            .collect();
        let mut it = nets.into_iter();
        match (it.next(), it.next()) {
            (Some(Some(n)), None) => Some(n),
            _ => None,
        }
    }

    pub(crate) fn length_info(&self) -> Option<LengthInfo> {
        let net = self.length_net()?;
        let doc = self.pcb_doc()?;
        let current = net_length(doc, net);
        let status = length_status(doc, net);
        let preview = self
            .pcb_ed
            .tune_preview
            .as_ref()
            .filter(|p| p.net == Some(net));
        let s = self.pcb_ed.tune;
        Some(LengthInfo {
            net: self.net_name(Some(net)),
            current_mm: mm_str(current.round() as i64),
            target_mm: status.map(|s| mm_str(s.target.round() as i64)),
            tolerance_mm: status.map(|s| mm_str(s.tolerance.round() as i64)),
            difference_mm: status.map(|s| mm_str(s.difference().round() as i64)),
            predicted_mm: preview
                .and_then(|p| p.plan.as_ref())
                .map(|(_, added)| mm_str((current + added).round() as i64)),
            from_pair: status.is_some_and(|s| s.from_pair),
            within: status.is_some_and(|s| s.within()),
            partial: preview.is_some_and(|p| p.partial),
            problem: preview.and_then(|p| p.problem.clone()),
            has_target: doc.rules.length_targets.contains_key(&net),
            style: s.style.id().into(),
            amplitude_mm: mm_str(s.amplitude),
            pitch_mm: mm_str(s.pitch),
        })
    }

    /// The meander as an overlay, in the highlight colour with its clearance halo.
    pub(crate) fn tune_preview_shapes(&self) -> Vec<(Shape, Rgba)> {
        let Some(p) = &self.pcb_ed.tune_preview else {
            return Vec::new();
        };
        let Some((path, _)) = &p.plan else {
            return Vec::new();
        };
        let col = ps::layer_color(p.layer, self.theme_kind());
        let halo = Rgba::with_alpha(col.0[0], col.0[1], col.0[2], 70);
        let clearance = self.clearance_for(p.net);
        let mut out = Vec::new();
        for w in path.windows(2) {
            let seg = Segment::new(w[0], w[1]);
            out.push((
                Shape::Line {
                    segment: seg,
                    width: p.width + 2 * clearance,
                },
                halo,
            ));
            out.push((
                Shape::Line {
                    segment: seg,
                    width: p.width,
                },
                col,
            ));
        }
        out
    }

    /// Tuner settings and target actions from the IPC layer: `tune_style`, `tune_amplitude`,
    /// `tune_pitch`, `length_target` (`<mm>/<tolerance mm>` or `clear`).
    pub(crate) fn pcb_tune_command(
        &mut self,
        name: &str,
        arg: Option<&str>,
    ) -> Result<bool, String> {
        match name {
            "tune_style" => {
                self.pcb_ed.tune.style = TuneStyle::parse(arg.unwrap_or("").trim())
                    .ok_or("style is serpentine or accordion")?;
                Ok(true)
            }
            "tune_amplitude" => {
                let v = parse_mm(arg.unwrap_or(""), "amplitude")?;
                if !(200_000..=50_000_000).contains(&v) {
                    return Err("amplitude must be between 0.2 mm and 50 mm".into());
                }
                self.pcb_ed.tune.amplitude = v;
                Ok(true)
            }
            "tune_pitch" => {
                let v = parse_mm(arg.unwrap_or(""), "pitch")?;
                if !(200_000..=20_000_000).contains(&v) {
                    return Err("pitch must be between 0.2 mm and 20 mm".into());
                }
                self.pcb_ed.tune.pitch = v;
                Ok(true)
            }
            "length_target" => {
                self.set_length_target(arg.unwrap_or("").trim())?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn set_length_target(&mut self, arg: &str) -> Result<(), String> {
        let net = self
            .length_net()
            .ok_or("select a track (or point at one with the tuner) to choose the net")?;
        let before = self.pcb_doc().ok_or("no project is open")?.rules.clone();
        let mut after = before.clone();
        if arg.is_empty() || arg == "clear" {
            after.length_targets.remove(&net);
        } else {
            let (t, tol) = arg.split_once('/').unwrap_or((arg, "0.1"));
            after.length_targets.insert(
                net,
                LengthTarget {
                    target: parse_mm(t, "target length")?,
                    tolerance: parse_mm(tol, "tolerance")?,
                },
            );
        }
        if after == before {
            return Ok(());
        }
        let open = self.open.as_mut().ok_or("no project is open")?;
        open.engine
            .execute(SetRulesCommand::new("Set length target", before, after))
            .map_err(|e| e.to_string())?;
        self.command_log.push("Set length target".into());
        self.pcb_ed.tune_preview = None;
        Ok(())
    }
}
