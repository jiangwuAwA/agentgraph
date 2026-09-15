import { authenticate, createUser } from "./auth";

export function loginHandler(email: string, password: string) {
  const ok = authenticate(email, password);
  if (!ok) {
    return { status: 401, body: "unauthorized" };
  }
  const user = createUser(email, password);
  return { status: 200, body: user };
}

export function bootstrap() {
  return loginHandler("a@b.com", "secret");
}
