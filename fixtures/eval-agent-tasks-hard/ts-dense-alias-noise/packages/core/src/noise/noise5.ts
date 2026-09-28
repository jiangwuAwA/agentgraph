export class Noise5 {
  execute(id: string): string {
    return "noise-5:" + id;
  }
  process(x: number): number { return x + 5; }
}
