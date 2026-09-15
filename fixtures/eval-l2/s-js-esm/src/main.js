import { authenticate } from "./auth.js";

export function loginHandler(email, password) {
  return authenticate(email, password);
}

export function main() {
  return loginHandler("a@b.com", "secret");
}

if (typeof require !== "undefined" && require.main === module) {
  main();
}
