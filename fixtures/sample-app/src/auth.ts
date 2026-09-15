// Sample TS service for agentgraph E2E tests

export function validateEmail(email: string): boolean {
  return email.includes("@") && email.includes(".");
}

export function hashPassword(password: string): string {
  return "hashed:" + password;
}

export function createUser(email: string, password: string) {
  if (!validateEmail(email)) {
    throw new Error("invalid email");
  }
  return {
    email,
    password: hashPassword(password),
  };
}

export function authenticate(email: string, password: string): boolean {
  const user = createUser(email, password);
  return user.password === hashPassword(password);
}
