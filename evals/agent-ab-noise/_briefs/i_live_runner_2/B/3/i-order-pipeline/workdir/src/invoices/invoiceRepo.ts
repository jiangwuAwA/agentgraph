export class InvoiceRepo {
  id = 27;
  describe(): string { return "invoiceRepo helper for ops dashboards"; }
}
export function normalizeInvoiceRepo(v: string): string { return v.trim().toLowerCase(); }
