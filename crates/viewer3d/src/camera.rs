//! Orbit camera with perspective and orthographic projection.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    Perspective,
    Orthographic,
}

/// An orbit camera looking at `target` (mm). `yaw` turns around Z, `pitch` is the elevation
/// above the board plane (90° = straight down); `distance` is the eye distance (mm), which also
/// sets the visible extent in orthographic mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub target: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub projection: Projection,
    /// Vertical field of view in radians (perspective).
    pub fov: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            yaw: -PI / 4.0,
            pitch: 0.6,
            distance: 100.0,
            projection: Projection::Perspective,
            fov: 45f64.to_radians(),
        }
    }
}

const MIN_PITCH: f64 = -PI / 2.0 + 0.01;
const MAX_PITCH: f64 = PI / 2.0 - 0.001;

impl Camera {
    /// Frames a board of the given extent (mm) from the default direction.
    pub fn framing(min: [f64; 2], max: [f64; 2]) -> Self {
        let size = (max[0] - min[0]).hypot(max[1] - min[1]).max(5.0);
        Self {
            target: [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, 0.0],
            distance: size * 1.1,
            ..Self::default()
        }
    }

    pub fn orbit(&mut self, dyaw: f64, dpitch: f64) {
        self.yaw = (self.yaw + dyaw).rem_euclid(2.0 * PI);
        self.pitch = (self.pitch + dpitch).clamp(MIN_PITCH, MAX_PITCH);
    }

    pub fn zoom(&mut self, factor: f64) {
        self.distance = (self.distance * factor).clamp(1.0, 5000.0);
    }

    /// Pans in the screen plane by `(dx, dy)` fractions of the view height.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let (right, up, _) = self.axes();
        let h = self.view_height();
        for i in 0..3 {
            self.target[i] += (-dx * right[i] + dy * up[i]) * h;
        }
    }

    /// Visible height at the target distance (mm).
    pub fn view_height(&self) -> f64 {
        match self.projection {
            Projection::Perspective => 2.0 * self.distance * (self.fov / 2.0).tan(),
            Projection::Orthographic => self.distance,
        }
    }

    pub fn set_view(&mut self, yaw: f64, pitch: f64) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(MIN_PITCH, MAX_PITCH);
    }

    /// Camera basis `(right, up, forward)` in world space (forward points from the eye to the
    /// target).
    pub fn axes(&self) -> ([f64; 3], [f64; 3], [f64; 3]) {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // The eye sits at target + distance * (cp*cy, cp*sy, sp)... looking back at it.
        let forward = [-cp * cy, -cp * sy, -sp];
        let right = [-sy, cy, 0.0];
        let up = [
            right[1] * forward[2] - right[2] * forward[1],
            right[2] * forward[0] - right[0] * forward[2],
            right[0] * forward[1] - right[1] * forward[0],
        ];
        (right, up, forward)
    }

    pub fn eye(&self) -> [f64; 3] {
        let (_, _, f) = self.axes();
        [
            self.target[0] - f[0] * self.distance,
            self.target[1] - f[1] * self.distance,
            self.target[2] - f[2] * self.distance,
        ]
    }

    /// Projects a world point to `(x, y, depth)` with x, y in pixels (y down) for a `w × h`
    /// image; `None` when the point is behind the eye.
    pub fn project(&self, p: [f64; 3], w: f64, h: f64) -> Option<(f64, f64, f64)> {
        let (r, u, f) = self.axes();
        let e = self.eye();
        let d = [p[0] - e[0], p[1] - e[1], p[2] - e[2]];
        let (x, y, z) = (
            d[0] * r[0] + d[1] * r[1] + d[2] * r[2],
            d[0] * u[0] + d[1] * u[1] + d[2] * u[2],
            d[0] * f[0] + d[1] * f[1] + d[2] * f[2],
        );
        let vh = self.view_height();
        let (sx, sy) = match self.projection {
            Projection::Perspective => {
                if z <= 0.01 {
                    return None;
                }
                let k = self.distance / z;
                (x * k / vh, y * k / vh)
            }
            Projection::Orthographic => (x / vh, y / vh),
        };
        Some((w / 2.0 + sx * h, h / 2.0 - sy * h, z))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_target_projects_to_the_centre_in_both_projections() {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let c = Camera {
                projection,
                ..Camera::default()
            };
            let (x, y, _) = c.project(c.target, 800.0, 600.0).unwrap();
            assert!((x - 400.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6);
        }
    }

    #[test]
    fn top_view_has_x_to_the_right_and_y_up_and_orbit_is_clamped() {
        let mut c = Camera::default();
        c.set_view(-PI / 2.0, PI / 2.0 - 0.001);
        let o = c.project([0.0, 0.0, 0.0], 800.0, 600.0).unwrap();
        let px = c.project([10.0, 0.0, 0.0], 800.0, 600.0).unwrap();
        let py = c.project([0.0, 10.0, 0.0], 800.0, 600.0).unwrap();
        assert!(px.0 > o.0 && (px.1 - o.1).abs() < 1.0, "+X goes right");
        assert!(
            py.1 < o.1 && (py.0 - o.0).abs() < 1.0,
            "+Y goes up the screen"
        );
        c.orbit(0.0, 10.0);
        assert!(c.pitch < PI / 2.0);
        c.orbit(0.0, -10.0);
        assert!(c.pitch > -PI / 2.0);
    }

    #[test]
    fn zoom_and_pan_move_the_view() {
        let mut c = Camera::default();
        let d = c.distance;
        c.zoom(0.5);
        assert!(c.distance < d);
        let t = c.target;
        c.pan(0.1, 0.0);
        assert_ne!(c.target, t);
    }
}
