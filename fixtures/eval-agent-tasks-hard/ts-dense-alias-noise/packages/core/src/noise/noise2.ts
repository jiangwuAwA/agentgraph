export class Noise2 {
  execute(id: string): string {
    return "noise-2:" + id;
  }
  process(x: number): number { return x + 2; }
}
