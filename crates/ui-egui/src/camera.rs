//! An orbit camera for the bed view: Z up, perspective, looking at a target.
//!
//! Pure math on `f64` so it can be tested without a window. Screen space is
//! egui's: origin top-left, Y down.

use slicecraft_geom::Vec3;

/// Pitch stays short of straight up or down, so "up" is always defined.
const MIN_PITCH: f64 = -1.55;
const MAX_PITCH: f64 = 1.55;
/// Nearest depth that is drawn, mm.
pub const NEAR: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians about Z, measured from +X.
    pub yaw: f64,
    /// Radians above the bed plane.
    pub pitch: f64,
    pub distance: f64,
    /// Vertical field of view, radians.
    pub fov: f64,
}

/// A point on screen with its depth along the view direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projected {
    pub x: f64,
    pub y: f64,
    pub depth: f64,
}

/// The rectangle being drawn into, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Camera basis for one frame: compute once, project many points.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    focal: f64,
    cx: f64,
    cy: f64,
}

/// Standard directions to look at the bed from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewPreset {
    Iso,
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

impl ViewPreset {
    pub const ALL: [ViewPreset; 7] = [
        ViewPreset::Iso,
        ViewPreset::Front,
        ViewPreset::Back,
        ViewPreset::Left,
        ViewPreset::Right,
        ViewPreset::Top,
        ViewPreset::Bottom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ViewPreset::Iso => "Perspective",
            ViewPreset::Front => "Front",
            ViewPreset::Back => "Back",
            ViewPreset::Left => "Left",
            ViewPreset::Right => "Right",
            ViewPreset::Top => "Top",
            ViewPreset::Bottom => "Bottom",
        }
    }

    /// (yaw, pitch) in radians.
    fn angles(self) -> (f64, f64) {
        use std::f64::consts::{FRAC_PI_2, PI};
        match self {
            ViewPreset::Iso => (-FRAC_PI_2 - 0.45, 0.62),
            ViewPreset::Front => (-FRAC_PI_2, 0.0),
            ViewPreset::Back => (FRAC_PI_2, 0.0),
            ViewPreset::Left => (PI, 0.0),
            ViewPreset::Right => (0.0, 0.0),
            ViewPreset::Top => (-FRAC_PI_2, MAX_PITCH),
            ViewPreset::Bottom => (-FRAC_PI_2, MIN_PITCH),
        }
    }
}

impl OrbitCamera {
    /// Look from a preset direction, keeping target and distance.
    pub fn look(&mut self, preset: ViewPreset) {
        let (yaw, pitch) = preset.angles();
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Centre on a box and back off until it fits the view.
    pub fn fit(&mut self, min: Vec3, max: Vec3) {
        let c = (min + max).scale(0.5);
        let radius = ((max - min).length() / 2.0).max(5.0);
        if c.is_in_range() && radius.is_finite() {
            self.target = c;
            self.distance = (radius / (self.fov / 2.0).sin() * 1.1).clamp(5.0, 20_000.0);
        }
    }

    /// A camera framing a bed of `width` × `depth` from the front-left, above.
    pub fn for_bed(width: f64, depth: f64, height: f64) -> Self {
        let span = width.max(depth).max(height * 0.6).max(10.0);
        Self {
            target: Vec3::new(width / 2.0, depth / 2.0, 0.0),
            yaw: -std::f64::consts::FRAC_PI_2 - 0.45,
            pitch: 0.62,
            distance: span * 1.4,
            fov: 40f64.to_radians(),
        }
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(cp * cy, cp * sy, sp).scale(self.distance)
    }

    pub fn orbit(&mut self, d_yaw: f64, d_pitch: f64) {
        if d_yaw.is_finite() {
            self.yaw = (self.yaw + d_yaw) % std::f64::consts::TAU;
        }
        if d_pitch.is_finite() {
            self.pitch = (self.pitch + d_pitch).clamp(MIN_PITCH, MAX_PITCH);
        }
    }

    /// Multiply the distance by `factor`, kept within sensible bounds.
    pub fn zoom(&mut self, factor: f64) {
        if factor.is_finite() && factor > 0.0 {
            self.distance = (self.distance * factor).clamp(5.0, 20_000.0);
        }
    }

    /// Move the target in the camera's screen plane by pixels.
    pub fn pan(&mut self, dx_px: f64, dy_px: f64, viewport: Viewport) {
        let f = self.frame(viewport);
        let mm_per_px = self.distance / f.focal.max(1e-9);
        let delta = f.right.scale(-dx_px * mm_per_px) + f.up.scale(dy_px * mm_per_px);
        let moved = self.target + delta;
        if moved.is_in_range() {
            self.target = moved;
        }
    }

    pub fn frame(&self, viewport: Viewport) -> Frame {
        let eye = self.eye();
        let forward = (self.target - eye).normalized().unwrap_or(Vec3::new(0.0, 1.0, 0.0));
        let right = forward
            .cross(Vec3::new(0.0, 0.0, 1.0))
            .normalized()
            .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let up = right.cross(forward);
        let focal = (viewport.height.max(1.0) / 2.0) / (self.fov / 2.0).tan();
        Frame {
            eye,
            right,
            up,
            forward,
            focal,
            cx: viewport.left + viewport.width / 2.0,
            cy: viewport.top + viewport.height / 2.0,
        }
    }
}

impl Frame {
    /// The camera position.
    pub fn eye(&self) -> Vec3 {
        self.eye
    }

    /// Screen position and depth of `p`, or `None` when it is behind the near plane.
    pub fn project(&self, p: Vec3) -> Option<Projected> {
        let v = p - self.eye;
        let depth = v.dot(self.forward);
        if depth < NEAR || !depth.is_finite() {
            return None;
        }
        Some(Projected {
            x: self.cx + v.dot(self.right) / depth * self.focal,
            y: self.cy - v.dot(self.up) / depth * self.focal,
            depth,
        })
    }

    /// Clip a segment to the near plane, then project both ends.
    pub fn project_segment(&self, a: Vec3, b: Vec3) -> Option<(Projected, Projected)> {
        let da = (a - self.eye).dot(self.forward);
        let db = (b - self.eye).dot(self.forward);
        if da < NEAR * 1.001 && db < NEAR * 1.001 {
            return None;
        }
        // cut a hair in front of the plane so rounding cannot put the cut behind it
        let clip = NEAR * 1.001;
        let cut = |p: Vec3, q: Vec3, dp: f64, dq: f64| p + (q - p).scale((clip - dp) / (dq - dp));
        let (a, b) = match (da < NEAR, db < NEAR) {
            (true, false) => (cut(a, b, da, db), b),
            (false, true) => (a, cut(b, a, db, da)),
            _ => (a, b),
        };
        Some((self.project(a)?, self.project(b)?))
    }

    /// Pixels per millimetre at `depth`.
    pub fn scale_at(&self, depth: f64) -> f64 {
        if depth > 0.0 { self.focal / depth } else { 0.0 }
    }

    /// The light direction for simple shading: from over the viewer's shoulder.
    pub fn light(&self) -> Vec3 {
        (self.up.scale(0.6) - self.forward + self.right.scale(-0.3))
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0))
    }

    /// Where the screen point hits the bed plane z = 0, if it does.
    pub fn bed_point(&self, x: f64, y: f64) -> Option<Vec3> {
        let dir = self.forward.scale(self.focal)
            + self.right.scale(x - self.cx)
            + self.up.scale(self.cy - y);
        if dir.z.abs() < 1e-12 {
            return None;
        }
        let t = -self.eye.z / dir.z;
        if t <= 0.0 || !t.is_finite() {
            return None;
        }
        Some(self.eye + dir.scale(t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VP: Viewport = Viewport { left: 0.0, top: 0.0, width: 800.0, height: 600.0 };

    #[test]
    fn target_projects_to_centre() {
        let cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        let p = cam.frame(VP).project(cam.target);
        assert!(p.is_some_and(|p| (p.x - 400.0).abs() < 1e-6 && (p.y - 300.0).abs() < 1e-6));
    }

    #[test]
    fn higher_points_are_higher_on_screen() {
        let cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        let f = cam.frame(VP);
        let (Some(low), Some(high)) =
            (f.project(cam.target), f.project(cam.target + Vec3::new(0.0, 0.0, 50.0)))
        else {
            return;
        };
        assert!(high.y < low.y);
    }

    #[test]
    fn behind_the_camera_is_not_drawn_and_segments_clip() {
        let cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        let f = cam.frame(VP);
        let behind = cam.eye() + (cam.eye() - cam.target);
        assert!(f.project(behind).is_none());
        assert!(f.project_segment(behind, cam.target).is_some());
        assert!(f.project_segment(behind, behind).is_none());
    }

    #[test]
    fn pitch_is_clamped_and_zoom_bounded() {
        let mut cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        cam.orbit(0.0, 10.0);
        assert!(cam.pitch <= MAX_PITCH);
        cam.orbit(f64::NAN, -10.0);
        assert!(cam.pitch >= MIN_PITCH && cam.yaw.is_finite());
        cam.zoom(1e-9);
        assert!(cam.distance >= 5.0);
        cam.zoom(f64::INFINITY);
        assert!(cam.distance.is_finite());
    }

    #[test]
    fn screen_centre_ray_hits_the_target_on_the_bed() {
        let mut cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        let hit = cam.frame(VP).bed_point(400.0, 300.0);
        assert!(hit.is_some_and(|h| (h - cam.target).length() < 1e-6));
        cam.look(ViewPreset::Top);
        let hit = cam.frame(VP).bed_point(400.0, 300.0);
        assert!(hit.is_some_and(|h| (h - cam.target).length() < 1e-6));
    }

    #[test]
    fn presets_and_fit() {
        let mut cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        for p in ViewPreset::ALL {
            cam.look(p);
            let f = cam.frame(VP);
            assert!(f.project(cam.target).is_some(), "{p:?} sees its target");
        }
        cam.fit(Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0));
        assert_eq!(cam.target, Vec3::new(10.0, 10.0, 10.0));
        let f = cam.frame(VP);
        for corner in [Vec3::new(0.0, 0.0, 0.0), Vec3::new(20.0, 20.0, 20.0)] {
            let p = f.project(corner);
            assert!(
                p.is_some_and(|p| (0.0..=800.0).contains(&p.x) && (0.0..=600.0).contains(&p.y))
            );
        }
    }

    #[test]
    fn panning_moves_the_target() {
        let mut cam = OrbitCamera::for_bed(220.0, 220.0, 250.0);
        let before = cam.target;
        cam.pan(100.0, 0.0, VP);
        assert!((cam.target - before).length() > 1.0);
    }
}
