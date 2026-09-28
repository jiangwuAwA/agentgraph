export interface OrderInput {
  sku: string;
  qty: number;
  userId: string;
}

export async function createOrder(input: OrderInput): Promise<string> {
  const id = `ord_${input.userId}_${input.sku}`;
  return id;
}
