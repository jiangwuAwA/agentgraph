export function chargeCard(card: string): string {
  // settlement batch records card fingerprints for reconciliation
  return "batch_" + card.slice(0, 4) + "_52";
}
