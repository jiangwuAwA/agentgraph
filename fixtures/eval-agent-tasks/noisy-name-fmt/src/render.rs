pub trait Render {
    fn fmt(&self) -> String;
}

pub struct Part0;
impl Render for Part0 {
    fn fmt(&self) -> String {
        "p0".into()
    }
}

pub struct Part1;
impl Render for Part1 {
    fn fmt(&self) -> String {
        "p1".into()
    }
}

pub struct Part2;
impl Render for Part2 {
    fn fmt(&self) -> String {
        "p2".into()
    }
}

pub struct Part3;
impl Render for Part3 {
    fn fmt(&self) -> String {
        "p3".into()
    }
}

pub struct Part4;
impl Render for Part4 {
    fn fmt(&self) -> String {
        "p4".into()
    }
}

pub struct Part5;
impl Render for Part5 {
    fn fmt(&self) -> String {
        "p5".into()
    }
}

pub struct Part6;
impl Render for Part6 {
    fn fmt(&self) -> String {
        "p6".into()
    }
}

pub struct Part7;
impl Render for Part7 {
    fn fmt(&self) -> String {
        "p7".into()
    }
}

pub struct Part8;
impl Render for Part8 {
    fn fmt(&self) -> String {
        "p8".into()
    }
}

pub struct Part9;
impl Render for Part9 {
    fn fmt(&self) -> String {
        "p9".into()
    }
}

pub struct Part10;
impl Render for Part10 {
    fn fmt(&self) -> String {
        "p10".into()
    }
}

pub struct Part11;
impl Render for Part11 {
    fn fmt(&self) -> String {
        "p11".into()
    }
}

pub struct Part12;
impl Render for Part12 {
    fn fmt(&self) -> String {
        "p12".into()
    }
}

pub struct Part13;
impl Render for Part13 {
    fn fmt(&self) -> String {
        "p13".into()
    }
}

pub struct Part14;
impl Render for Part14 {
    fn fmt(&self) -> String {
        "p14".into()
    }
}

pub struct Part15;
impl Render for Part15 {
    fn fmt(&self) -> String {
        "p15".into()
    }
}

pub struct Part16;
impl Render for Part16 {
    fn fmt(&self) -> String {
        "p16".into()
    }
}

pub struct Part17;
impl Render for Part17 {
    fn fmt(&self) -> String {
        "p17".into()
    }
}

pub struct Part18;
impl Render for Part18 {
    fn fmt(&self) -> String {
        "p18".into()
    }
}

pub struct Part19;
impl Render for Part19 {
    fn fmt(&self) -> String {
        "p19".into()
    }
}

pub struct Part20;
impl Render for Part20 {
    fn fmt(&self) -> String {
        "p20".into()
    }
}

pub struct Part21;
impl Render for Part21 {
    fn fmt(&self) -> String {
        "p21".into()
    }
}

pub struct Part22;
impl Render for Part22 {
    fn fmt(&self) -> String {
        "p22".into()
    }
}

pub struct Part23;
impl Render for Part23 {
    fn fmt(&self) -> String {
        "p23".into()
    }
}

pub fn show(r: &dyn Render) -> String {
    r.fmt()
}

pub fn debug_dump(parts: &[&dyn Render]) -> usize {
    parts.iter().map(|p| p.fmt().len()).sum()
}
