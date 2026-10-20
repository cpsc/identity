# Zeabur design system (vendored)

The server-rendered entry of [Zeabur's design system](https://github.com/zeabur/design-system):
its tokens, type roles, utilities and base styles, without components, `@font-face` or `url()`.
`ui/styles/app.css` imports `tokens.css` right after Tailwind CSS.

- Source: `github.com/zeabur/design-system` at commit
  `d5ef81dbd9de89250914722005e97887bcd9c615`.
- Files: `tokens.css`, `src/tokens.css`, `src/utilities.css`, `src/base.css` and `LICENSE`, copied
  verbatim. Update them only as a whole, from a newer commit, and change the commit above.
- Licence: Apache License 2.0 (`LICENSE`).

Also taken from the same commit, outside this directory:

- `crates/zeabur-identity-ui/assets/Geist-Variable.woff2` and `LICENSE-Geist.txt`: the design
  system's `vendor/geist` (Geist by Vercel with basement.studio, SIL Open Font License 1.1).
- The Zeabur mark in `crates/zeabur-identity-ui/templates/brand.html`: the paths of the design
  system's `Mark` component (`src/components/brand.tsx`, from `vendor/zeabur/logo.svg`). The
  Zeabur name, logo and mark are trademarks of Zeabur Inc. and are not licensed under the Apache
  License 2.0; Zeabur Identity is a Zeabur product and shows them as such.
