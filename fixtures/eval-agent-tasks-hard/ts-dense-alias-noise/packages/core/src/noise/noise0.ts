export class Noise0 {
  execute(id: string): string {
    return "noise-0:" + id;
  }
  process(x: number): number { return x + 0; }
}
