use crate::handlers::{Handler, HtmlHandler, JsonHandler};

pub mod handlers;
pub mod metrics;

pub fn dispatch(h: &dyn Handler) -> String {
    h.render()
}

pub fn bootstrap() -> String {
    let handlers: Vec<Box<dyn Handler>> = vec![Box::new(HtmlHandler), Box::new(JsonHandler)];
    handlers.iter().map(|h| h.render()).collect()
}
