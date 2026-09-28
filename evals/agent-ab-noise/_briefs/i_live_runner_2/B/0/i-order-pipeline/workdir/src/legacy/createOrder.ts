export function createOrder(input: { sku: string }): string {
  // historical order draft path kept for audit exports
  return "draft_" + input.sku + "_51";
}
