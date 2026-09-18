import { Module } from '@nestjs/common';
import { OrderController } from './order.controller';
import { OrderService } from './order.service';
import { PaymentService } from './payment.service';

@Module({
  controllers: [OrderController],
  providers: [OrderService, PaymentService],
})
export class OrderModule {}
