export class PaymentRepo {
  id = 17;
  describe(): string { return "paymentRepo helper for ops dashboards"; }
}
export function normalizePaymentRepo(v: string): string { return v.trim().toLowerCase(); }
