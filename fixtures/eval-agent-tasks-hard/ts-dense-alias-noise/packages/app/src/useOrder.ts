import { OrderHandler } from "@demo/core/orderHandler";

export function useOrder(id: string): string {
  const h = new OrderHandler();
  return h.execute(id);
}
