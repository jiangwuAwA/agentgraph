export class InvoiceTypes {
  id = 25;
  describe(): string { return "invoiceTypes helper for ops dashboards"; }
}
export function normalizeInvoiceTypes(v: string): string { return v.trim().toLowerCase(); }
