trait T {
    fn fmt(&self) -> String;
}

struct S0;
struct S1;
struct S2;
struct S3;
struct S4;
struct S5;
struct S6;
struct S7;
struct S8;
struct S9;
struct S10;
struct S11;
struct S12;
struct S13;
struct S14;
struct S15;
struct S16;
struct S17;
struct S18;
struct S19;
struct S20;
struct S21;
struct S22;
struct S23;
struct S24;

impl T for S0 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S1 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S2 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S3 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S4 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S5 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S6 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S7 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S8 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S9 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S10 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S11 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S12 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S13 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S14 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S15 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S16 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S17 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S18 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S19 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S20 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S21 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S22 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S23 {
    fn fmt(&self) -> String {
        "x".into()
    }
}
impl T for S24 {
    fn fmt(&self) -> String {
        "x".into()
    }
}

fn show(t: &dyn T) -> String {
    t.fmt()
}

fn main() {
    let _ = show(&S0);
}
