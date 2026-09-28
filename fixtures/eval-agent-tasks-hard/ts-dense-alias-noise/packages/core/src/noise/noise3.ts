export class Noise3 {
  execute(id: string): string {
    return "noise-3:" + id;
  }
  process(x: number): number { return x + 3; }
}
