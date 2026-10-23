---
name: zeabur-identity-ui
description: Change Zeabur Identity's sign-in pages — askama templates, TypeScript, Tailwind CSS, translations — while keeping the strict CSP, committed reproducible assets, size budgets and WCAG 2.2 AA contrast. Use for any change under ui/ or crates/zeabur-identity-ui.
---

# Sign-in pages

- **Where things are.** Sources: `ui/src/*.ts` (scripts), `ui/styles/*.css` (Tailwind v4 input).
  Templates: `crates/zeabur-identity-ui/templates/` (askama; Zeabur design system class strings, from
  the vendored `ui/styles/vendor/zeabur-design-system/`). Built output:
  `crates/zeabur-identity-ui/assets/` (committed, embedded with `include_bytes!`). Page routes live in
  `crates/zeabur-identity-signin/src/pages.rs`; rendering in `crates/zeabur-identity-ui/src/pages.rs`.
- **Build and check** (inside `nix develop`):
  ```sh
  ./scripts/build-ui.sh && git diff --stat -- crates/zeabur-identity-ui/assets
  tsc -p ui && ./scripts/ui-budget.sh
  ./scripts/fallow.sh dead-code --baseline fallow-baseline.json && ./scripts/fallow.sh audit --base main
  cargo nextest run -p zeabur-identity-signin -E 'binary(pages)'
  ```
  Commit the rebuilt assets with the source change; CI rebuilds and fails on any difference.
- **fallow** (`scripts/fallow.sh <command>`, config in `ui/.fallowrc.json`). `dead-code` finds
  unused files and exports, and a page that imports another page (pages import only
  `webauthn.ts`). `ui/fallow-baseline.json` holds the findings that predate fallow; never add to
  it, and run `scripts/fallow.sh baselines prune` after a fix. `audit --base main` is what CI
  gates on: new dead code, complexity and duplication in the changed files. `health` gives the
  score and the complex functions; with no unit tests for `ui/src`, CRAP counts every branch as
  untested. A false positive gets an entry in `.fallowrc.json` with the reason.
- **CSP rules.** No inline `<script>`, `<style>`, `style=""` or `on*=` handlers; no URLs to other
  origins; scripts write only `textContent`, `hidden` and `location` (Trusted Types `'none'`). The
  page tests enforce this.
- **Text.** Every string is a field of `Messages` in `crates/zeabur-identity-ui/src/i18n.rs` with English,
  Traditional Chinese (Taiwan usage, Passkey) and Simplified Chinese (mainland usage, 通行密钥)
  values; scripts get their messages from `data-msg-*` attributes. A missing translation is a
  compile error. Wording and terms follow Zeabur's `docs/wording.md`: sentence case, errors
  that say what failed and what to do next, no "we"; in Chinese no 被／進行／您, 請 only in a
  full sentence that asks the reader to act.
- **Contrast.** After changing a token or a colour class, run
  `python3 .claude/skills/zeabur-identity-ui/contrast.py` (inside `nix develop`; exits 1 on any
  failure). Try a value first with `--set light:input='rgb(126, 122, 133)'`. Add a pair to its
  inventory when a template starts rendering a new one.
- **Budgets.** Brotli sizes are checked by `scripts/ui-budget.sh`; raise a budget only with a
  reason in the commit.
