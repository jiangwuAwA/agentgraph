import { createOrder, OrderInput } from "./createOrder";
import { chargeCard } from "../payments/chargeCard";

export class OrderService {
  async place(input: OrderInput, card: string): Promise<string> {
    const id = await createOrder(input);
    await chargeCard(card, input.qty * 10);
    return id;
  }
}
