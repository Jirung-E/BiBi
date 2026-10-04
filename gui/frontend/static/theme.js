// Apply the saved choice before CSS/app startup, without weakening script CSP.
// Keep the key and fallback in sync with src/lib/theme.ts.
(function () {
  var choice = 'system';
  try { choice = localStorage.getItem('bibi:theme'); } catch (_) { /* System default. */ }
  var effective = choice === 'light' || choice === 'dark' ? choice
    : window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  document.documentElement.dataset.theme = effective;
  document.documentElement.style.colorScheme = effective;
})();
