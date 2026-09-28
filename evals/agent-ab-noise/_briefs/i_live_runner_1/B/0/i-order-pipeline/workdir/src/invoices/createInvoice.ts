export class CreateInvoice {
  id = 26;
  describe(): string { return "createInvoice helper for ops dashboards"; }
}
export function normalizeCreateInvoice(v: string): string { return v.trim().toLowerCase(); }
