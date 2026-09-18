export class RegistryClient {
  constructor(private endpoint: string) {}

  fetch(id: string): string {
    return `${this.endpoint}/${id}`;
  }
}

export function createClient(endpoint: string): RegistryClient {
  return new RegistryClient(endpoint);
}
