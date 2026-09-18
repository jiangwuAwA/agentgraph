//! Same-file dyn + impls so `rs.di.dyn_trait_method` can fire (M3-A).
//! Cross-crate dispatch is intentionally NOT modeled (open domain).

pub trait Handler {
    fn render(&self) -> String;
}

pub struct HtmlHandler;
pub struct JsonHandler;

impl Handler for HtmlHandler {
    fn render(&self) -> String {
        "<html/>".into()
    }
}

impl Handler for JsonHandler {
    fn render(&self) -> String {
        "{}".into()
    }
}

/// Call site on dyn — Heuristic implementor candidates for `render`.
pub fn render_dyn(h: &dyn Handler) -> String {
    h.render()
}
