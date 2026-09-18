import { OrderService } from "./order.service";

export class OrderController {
  constructor(private service: OrderService) {}

  get(id: string): string {
    return this.service.lookup(id);
  }
}
