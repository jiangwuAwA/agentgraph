import { AppService } from "./app.service";

export class AppController {
  constructor(private appService: AppService) {}
  getHello(): string {
    return this.appService.getHello();
  }
}
