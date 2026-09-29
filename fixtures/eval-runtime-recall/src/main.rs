fn main() {
    let _ = rr_fixture::orders::create_order("a1");
    let _ = rr_fixture::orders::order_total(3);
}
