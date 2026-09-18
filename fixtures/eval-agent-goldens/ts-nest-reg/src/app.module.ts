import { AppController } from "./app.controller";
import { AppService } from "./app.service";

export class AppModule {}

export const moduleMeta = {
  controllers: [AppController],
  providers: [AppService],
};
