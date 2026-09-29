//! Runtime-recall fixture with explicit rr_edge probes.
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;
static TRACE_LOCK: Mutex<()> = Mutex::new(());
pub fn rr_edge(from: &str, to: &str, from_file: &str, from_line: u32, to_file: &str, to_line: u32) {
    let path = std::env::var("RR_TRACE").unwrap_or_else(|_| "target/rr_trace.jsonl".into());
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(_g) = TRACE_LOCK.lock() {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
            let line = format!(
                "{{\"schema\":\"agentgraph.eval_runtime_recall.edge.v1\",\"from\":\"{}\",\"to\":\"{}\",\"from_file\":\"{}\",\"from_line\":{},\"to_file\":\"{}\",\"to_line\":{}}}\n",
                from, to, from_file, from_line, to_file, to_line
            );
            let _ = f.write_all(line.as_bytes());
        }
    }
}
pub mod orders {
    pub fn create_order(id: &str) -> String {
        crate::rr_edge("orders::create_order", "payments::charge_card", "src/lib.rs", line!(), "src/lib.rs", 0);
        let charged = crate::payments::charge_card(id);
        format!("ord-{id}-{charged}")
    }
    pub fn order_total(n: i32) -> i32 {
        crate::rr_edge("orders::order_total", "orders::line_total", "src/lib.rs", line!(), "src/lib.rs", 0);
        line_total(n)
    }
    pub fn line_total(n: i32) -> i32 {
        n * 10
    }
}
pub mod payments {
    pub fn charge_card(id: &str) -> String {
        crate::rr_edge("payments::charge_card", "payments::tokenize", "src/lib.rs", line!(), "src/lib.rs", 0);
        let token = tokenize(id);
        format!("chg-{token}")
    }
    pub fn tokenize(id: &str) -> String {
        format!("tok_{id}")
    }
}
pub mod legacy {
    pub fn legacy_charge(id: &str) -> String {
        format!("legacy-{id}")
    }
}
