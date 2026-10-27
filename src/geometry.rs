use serde::{Deserialize, Serialize};

/// Rectangle in logical (compositor layout) coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_points(ax: f64, ay: f64, bx: f64, by: f64) -> Self {
        Self::new(ax.min(bx), ay.min(by), (ax - bx).abs(), (ay - by).abs())
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = self.right().min(o.right());
        let y2 = self.bottom().min(o.bottom());
        (x2 - x1 >= 1.0 && y2 - y1 >= 1.0).then(|| Rect::new(x1, y1, x2 - x1, y2 - y1))
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    pub fn relative_to(&self, origin: &Rect) -> Rect {
        self.translate(-origin.x, -origin.y)
    }

    /// Moves the rectangle so it lies inside `b`, shrinking it if it is larger.
    pub fn clamp_inside(&self, b: &Rect) -> Rect {
        let w = self.w.min(b.w);
        let h = self.h.min(b.h);
        Rect::new(
            self.x.clamp(b.x, b.right() - w),
            self.y.clamp(b.y, b.bottom() - h),
            w,
            h,
        )
    }

    pub fn round(&self) -> Rect {
        let x = self.x.round();
        let y = self.y.round();
        Rect::new(x, y, self.right().round() - x, self.bottom().round() - y)
    }

    /// Rounded with even width and height, which most video encoders require.
    pub fn even(&self) -> Rect {
        let r = self.round();
        let even = |v: f64| ((v as i64 / 2) * 2).max(2) as f64;
        Rect::new(r.x, r.y, even(r.w), even(r.h))
    }

    pub fn is_usable(&self) -> bool {
        self.w >= 2.0 && self.h >= 2.0
    }

    /// Geometry in the "x,y wxh" format understood by grim, slurp and wf-recorder.
    pub fn geometry(&self) -> String {
        let r = self.round();
        format!(
            "{},{} {}x{}",
            r.x as i64, r.y as i64, r.w as i64, r.h as i64
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn from_points_normalises_direction() {
        assert_eq!(
            Rect::from_points(10.0, 20.0, 4.0, 5.0),
            Rect::new(4.0, 5.0, 6.0, 15.0)
        );
    }

    #[test]
    fn intersect_and_contains() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);
        assert_eq!(a.intersect(&b), Some(Rect::new(50.0, 50.0, 50.0, 50.0)));
        assert_eq!(a.intersect(&Rect::new(200.0, 0.0, 10.0, 10.0)), None);
        assert!(a.contains(0.0, 99.9));
        assert!(!a.contains(100.0, 50.0));
    }

    #[test]
    fn clamp_inside_moves_and_shrinks() {
        let screen = Rect::new(0.0, 0.0, 1600.0, 900.0);
        assert_eq!(
            Rect::new(1500.0, -20.0, 300.0, 100.0).clamp_inside(&screen),
            Rect::new(1300.0, 0.0, 300.0, 100.0)
        );
        assert_eq!(
            Rect::new(10.0, 10.0, 2000.0, 50.0).clamp_inside(&screen),
            Rect::new(0.0, 10.0, 1600.0, 50.0)
        );
    }

    #[test]
    fn even_rounds_down_to_even_sizes() {
        assert_eq!(
            Rect::new(1.4, 2.6, 571.0, 291.2).even(),
            Rect::new(1.0, 3.0, 570.0, 290.0)
        );
        assert_eq!(Rect::new(0.0, 0.0, 1.0, 1.0).even().w, 2.0);
    }

    #[test]
    fn geometry_string() {
        assert_eq!(
            Rect::new(10.2, 20.7, 300.0, 200.0).geometry(),
            "10,21 300x200"
        );
    }
}
