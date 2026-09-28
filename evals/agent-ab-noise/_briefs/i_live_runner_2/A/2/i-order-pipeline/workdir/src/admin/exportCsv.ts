export class ExportCsv {
  id = 48;
  describe(): string { return "exportCsv helper for ops dashboards"; }
}
export function normalizeExportCsv(v: string): string { return v.trim().toLowerCase(); }
