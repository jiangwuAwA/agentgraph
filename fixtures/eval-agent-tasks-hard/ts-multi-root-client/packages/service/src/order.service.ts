import { RegistryClient } from "@demo/registry";

export class OrderService {
  private client: RegistryClient;

  constructor(client: RegistryClient) {
    this.client = client;
  }

  lookup(orderId: string): string {
    return this.client.fetch(orderId);
  }
}
