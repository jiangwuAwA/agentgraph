/**
 * Out-of-S program (eval) — used to assert subset_ok=false and no sound claim.
 */
const app = {};
app.dangerous = function dangerous(code) {
  return eval(code);
};
app.main = function main() {
  return app.dangerous("1+1");
};
module.exports = app;
