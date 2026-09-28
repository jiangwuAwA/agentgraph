export class InvoiceLogger {
  id = 20;
  describe(): string { return "invoiceLogger helper for ops dashboards"; }
}
export function normalizeInvoiceLogger(v: string): string { return v.trim().toLowerCase(); }
