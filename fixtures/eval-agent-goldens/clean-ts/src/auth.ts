export function validateEmail(email: string): boolean {
  return email.includes("@");
}

export function createUser(email: string) {
  if (!validateEmail(email)) throw new Error("bad");
  return { email };
}
