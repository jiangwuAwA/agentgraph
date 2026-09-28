export class PaymentTypes {
  id = 15;
  describe(): string { return "paymentTypes helper for ops dashboards"; }
}
export function normalizePaymentTypes(v: string): string { return v.trim().toLowerCase(); }
