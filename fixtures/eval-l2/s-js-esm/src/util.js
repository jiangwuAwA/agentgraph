/**
 * Multi-file ESM S_js corpus — differential fixture.
 * util.js → auth.js → main.js static import chain.
 */
export function normalize(email) {
  return String(email).trim().toLowerCase();
}
