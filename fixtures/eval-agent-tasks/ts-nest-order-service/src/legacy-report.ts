// Noise: report generator with no OrderService dependency.
export class ReportBuilder {
  build(rows: string[]) {
    return rows.join('\n');
  }
}
