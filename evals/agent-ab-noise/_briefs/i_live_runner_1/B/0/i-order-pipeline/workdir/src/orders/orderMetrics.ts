export const counters = { orders_created: 0, payments_captured: 0 };
export function bump(): void {
  // incremented after createOrder and chargeCard succeed
  counters.orders_created += 1;
}
