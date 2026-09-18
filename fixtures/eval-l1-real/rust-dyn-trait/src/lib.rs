//! Multi-file-style real-idiom corpus: Rust dyn Trait + impls (M3-A).
//! Multi-module layout with a sibling module; dyn dispatch rule is same-file.

use crate::handlers::{Handler, HtmlHandler, JsonHandler};

pub mod handlers;

pub fn dispatch(h: &dyn Handler) -> String {
    h.render()
}

pub fn run_all(list: &[Box<dyn Handler>]) -> String {
    list.iter().map(|h| h.render()).collect()
}

pub fn bootstrap() -> String {
    let handlers: Vec<Box<dyn Handler>> =
        vec![Box::new(HtmlHandler), Box::new(JsonHandler)];
    run_all(&handlers)
}
