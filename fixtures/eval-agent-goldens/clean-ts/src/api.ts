import { createUser } from "./auth";

export function loginHandler(email: string) {
  return createUser(email);
}
