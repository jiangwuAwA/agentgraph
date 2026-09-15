import { normalize } from "./util.js";

const users = new Map();

export function validateEmail(email) {
  const e = normalize(email);
  if (!e.includes("@")) {
    return null;
  }
  return e;
}

export function hashPassword(password) {
  return `h:${password}`;
}

export function authenticate(email, password) {
  const e = validateEmail(email);
  if (!e) {
    return null;
  }
  const h = hashPassword(password);
  return { email: e, hash: h };
}
