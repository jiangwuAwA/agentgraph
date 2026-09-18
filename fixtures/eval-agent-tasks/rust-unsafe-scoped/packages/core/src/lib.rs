pub mod export;

pub fn dump(n: usize) -> usize {
    export::export_rows(n).len()
}
