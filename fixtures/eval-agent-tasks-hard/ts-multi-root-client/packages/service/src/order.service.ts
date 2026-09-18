import { RegistryClient } from "@demo/registry";

export class OrderService {
  constructor(private client: RegistryClient) {}
  load(id: string) {
    return this.client.fetch(id);
  }
}
