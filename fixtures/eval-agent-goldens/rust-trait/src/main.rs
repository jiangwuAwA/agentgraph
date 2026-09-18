trait Shape {
    fn area(&self) -> f64;
}

struct Circle {
    r: f64,
}

struct Rect {
    w: f64,
    h: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.0
    }
}

impl Shape for Rect {
    fn area(&self) -> f64 {
        self.w * self.h
    }
}

fn paint(s: &dyn Shape) -> f64 {
    s.area()
}

fn main() {
    let c = Circle { r: 1.0 };
    let _ = paint(&c);
}
