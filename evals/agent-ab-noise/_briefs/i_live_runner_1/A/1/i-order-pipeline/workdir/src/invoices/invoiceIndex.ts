export class InvoiceIndex {
  id = 24;
  describe(): string { return "invoiceIndex helper for ops dashboards"; }
}
export function normalizeInvoiceIndex(v: string): string { return v.trim().toLowerCase(); }
