export class PaymentIndex {
  id = 14;
  describe(): string { return "paymentIndex helper for ops dashboards"; }
}
export function normalizePaymentIndex(v: string): string { return v.trim().toLowerCase(); }
