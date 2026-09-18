pub mod routes;

pub fn handle(raw: &str) -> String {
    routes::lookup(raw)
}
