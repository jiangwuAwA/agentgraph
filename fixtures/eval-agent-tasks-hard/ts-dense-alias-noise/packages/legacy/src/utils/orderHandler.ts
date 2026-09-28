// WRONG package — same class name, not the target
export class OrderHandler {
  execute(id: string): string {
    return "legacy-" + id;
  }
}
