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
  cargo nextest run -p zeabur-identity-signin -E 'binary(pages)'
  ```
  Commit the rebuilt assets with the source change; CI rebuilds and fails on any difference.
- **CSP rules.** No inline `<script>`, `<style>`, `style=""` or `on*=` handlers; no URLs to other
  origins; scripts write only `textContent`, `hidden` and `location` (Trusted Types `'none'`). The
  page tests enforce this.
- **Text.** Every string is a field of `Messages` in `crates/zeabur-identity-ui/src/i18n.rs` with English,
  Traditional Chinese (Taiwan usage, Passkey) and Simplified Chinese (mainland usage, 通行密钥)
  values; scripts get their messages from `data-msg-*` attributes. A missing translation is a
  compile error. Wording, element conventions and terms follow Zeabur's `docs/wording.md`
  (engawa issue #60): sentence case, errors that say what failed and what to do next, no "we";
  in Chinese no 被／進行／您, 請 only in a full sentence that asks the reader to act.
- **Contrast.** `contrast.py` in this skill reads the tokens from the vendored design system and
  `ui/styles/app.css` and checks
  every colour pair the pages render (text ≥ 4.5:1, boundaries and focus rings ≥ 3:1), with
  alpha compositing; it exits 1 on any failure. Run it after changing a token or a colour class:
  `python3 .claude/skills/zeabur-identity-ui/contrast.py` (inside `nix develop`). Try a value first with
  `--set light:input='rgb(126, 122, 133)'`. When a template starts rendering a new pair (a variant,
  state or surface), add it to the inventory in the script. Update the `A11Y` comments in
  `ui/styles/app.css` with the new ratios.
- **Budgets.** Brotli sizes are checked by `scripts/ui-budget.sh`; raise a budget only with a
  reason in the commit.
