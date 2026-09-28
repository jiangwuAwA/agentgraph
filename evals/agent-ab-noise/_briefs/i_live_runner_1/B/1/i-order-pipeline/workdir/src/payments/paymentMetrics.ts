export const counters = { orders_created: 0, payments_captured: 0, invoices_sent: 0 };
export function bump(name: keyof typeof counters): void { counters[name] += 1; void 11; }
