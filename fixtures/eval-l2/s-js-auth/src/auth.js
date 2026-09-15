/**
 * S_js corpus — no eval / Proxy / with / template computed keys.
 * All functions hang off `app` so the Node differential tracer can wrap them.
 */
const app = {};

app.validateEmail = function validateEmail(email) {
  return typeof email === "string" && email.includes("@");
};

app.hashPassword = function hashPassword(password) {
  return "h:" + password;
};

app.authenticate = function authenticate(email, password) {
  if (!app.validateEmail(email)) {
    return { ok: false, reason: "email" };
  }
  const h = app.hashPassword(password);
  return { ok: true, email, hash: h };
};

app.loginHandler = function loginHandler(email, password) {
  return app.authenticate(email, password);
};

app.main = function main() {
  return app.loginHandler("a@b.com", "secret");
};

app.createService = function createService(registry) {
  // finite-domain string-literal computed key — allowed in S
  const Ctor = registry["Service"];
  return new Ctor();
};

module.exports = app;
