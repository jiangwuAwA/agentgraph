import { Body, Controller, Post } from '@nestjs/common';
import { OrderService } from './order.service';

@Controller('orders')
export class OrderController {
  constructor(private readonly orders: OrderService) {}

  @Post()
  create(@Body() body: { userId: string; amount: number }) {
    return this.orders.createOrder(body.userId, body.amount);
  }
}
