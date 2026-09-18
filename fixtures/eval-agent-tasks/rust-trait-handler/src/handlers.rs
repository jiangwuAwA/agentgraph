//! Same-file dyn + implementors for `render`.
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

pub fn render_dyn(h: &dyn Handler) -> String {
    h.render()
}
