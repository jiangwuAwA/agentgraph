import { Injectable } from '@nestjs/common';
import { PaymentService } from './payment.service';

export interface OrderResult {
  id: string;
  total: number;
}

@Injectable()
export class OrderService {
  constructor(private readonly payments: PaymentService) {}

  createOrder(userId: string, amount: number): string {
    this.payments.charge(userId, amount);
    return `order-${userId}-${amount}`;
  }
}
