export interface Logger {
  info(msg: string): void;
  error(msg: string): void;
}

export class ConsoleLogger implements Logger {
  info(msg: string) {
    console.log(msg);
  }
  error(msg: string) {
    console.error(msg);
  }
}
