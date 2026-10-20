# zeabur-identity-ui

Zeabur Identity's sign-in pages: server-rendered HTML, embedded static assets, translations and the
security headers every page carries. Routing and the data behind each page belong to the
caller.

## Modules

- `pages`: renders the sign-in, enrolment, error and notice pages from askama templates in
  `templates/`. askama compiles templates at build time and escapes HTML by default, so the
  client name, `login_hint` and error details cannot inject markup.
- `assets`: serves the embedded CSS, scripts and font (Geist) at `/assets/{name}` by exact name, cached
  as immutable and versioned by the first 16 hex digits of their SHA-256.
- `i18n`: English, Traditional Chinese (Taiwan) and Simplified Chinese. The language is the
  `?lang=` switcher, then the client's `ui_locales` where the caller has it, then
  `Accept-Language` by quality ([RFC 9110 §12.5.4][rfc9110]), then English.
- `harden` and `CSP`: the response headers below.

Depends only on `zeabur-identity-core`. Used by `zeabur-identity-oauth` (the error page), `zeabur-identity-signin` (the
page routes) and the `zeabur-identity` server.

## Frontend source and build

The sources live in `ui/` at the repository root: `ui/styles/app.css` (the theme setup and local
accessibility overrides marked `A11Y`), `ui/styles/vendor/zeabur-design-system/` (the tokens,
utilities and base styles of Zeabur's design system, vendored; its README names the commit) and
the TypeScript in `ui/src/`. The pages look like Zeabur's own sign-in: the Zeabur mark over a
centred card, Geist, the violet gradient button and the white one, on zGray-50 (zGray-950 in
dark mode). `scripts/build-ui.sh` runs Tailwind CSS v4 over `app.css`,
which scans the templates for class names, and `bun build --minify` over `login.ts` and
`setup.ts`. The output in `assets/` is committed and embedded with `include_bytes!`, so
`cargo build` needs no Node, bun or Tailwind and nothing is loaded from another origin. CI
rebuilds the assets and fails if they differ from the committed files, typechecks the scripts,
and runs `scripts/ui-budget.sh`, which limits the brotli-compressed size to 8 KiB for `app.css`
and 3 KiB for each script. There is no `package.json` and no npm dependency.

The scripts POST JSON and navigate with `location.assign`; no page submits a form, so the CSP
can say `form-action 'none'`. They convert WebAuthn JSON by hand rather than with
`PublicKeyCredential.parse*FromJSON`, which older Safari versions lack. They write only
`textContent`, never HTML, take their messages from `data-msg-*` attributes in the page language,
store nothing in the browser and never see a token.

## Security headers

Every HTML page sets a CSP with `default-src 'none'`, `'self'` sources only, no inline script or
style ([CSP Level 3][csp]), `frame-ancestors 'none'` and Trusted Types with no policy
([Trusted Types][tt]), so any string-to-HTML sink throws. With `X-Frame-Options: DENY` this
meets the clickjacking defence of [RFC 9700 §4.16][rfc9700]. Pages also send `nosniff`,
`Referrer-Policy: no-referrer`, `Cache-Control: no-store`, same-origin COOP and CORP, and a
`Permissions-Policy` allowing only WebAuthn. The error page for an untrusted authorization
request is shown with status 400 and no redirect ([RFC 6749 §4.1.2.1][rfc6749]).

## Accessibility

The pages target [WCAG 2.2][wcag] AA, follow the operating system's dark mode without script,
honour reduced motion, and keep a focus outline in forced-colours mode. Contrast ratios are
computed from the design system's sRGB tokens (alpha composited in sRGB as browsers do, WCAG
relative luminance) by `.claude/skills/zeabur-identity-ui/contrast.py`; where the design system
falls short, `app.css` overrides it:

| Pair | Light | Dark | Needed |
|---|---|---|---|
| Text on card | 21.00 | 17.68 | 4.5 |
| Secondary text on card / on the page | 6.04 / 5.51 | 8.64 / 9.40 | 4.5 |
| Error text on card (`fail-strong`, not `destructive`: 3.86:1 light) | 6.49 | 9.40 | 4.5 |
| Elevation notice (warning alert) | 5.67 | 10.40 | 4.5 |
| Primary button text (lightest gradient stop) | 5.07 | 5.07 | 4.5 |
| Input border on card | 4.19 (upstream 1.19) | 4.22 (upstream 1.27) | 3 |
| Focus outline on card | 7.06 (upstream 2.27) | 5.09 (upstream 1.84) | 3 |

Button edges are not checked: their text and icon identify them (WCAG 1.4.11).

## Credits

Styles and class strings come from [Zeabur's design system][zds] (Apache-2.0), itself built on
[shadcn/ui][shadcn] (MIT License). The Zeabur mark is a trademark of Zeabur Inc. Icons are
inline Phosphor SVGs (MIT License). The [Geist][geist] font (the design system's copy) is
embedded under the SIL Open Font License 1.1 (`assets/LICENSE-Geist.txt`); Chinese text uses
the system's CJK fonts.

## References

- R. Fielding, M. Nottingham, and J. Reschke, Eds., "HTTP Semantics," RFC 9110, IETF,
  Jun. 2022. <https://www.rfc-editor.org/rfc/rfc9110>
- D. Hardt, Ed., "The OAuth 2.0 Authorization Framework," RFC 6749, IETF, Oct. 2012.
  <https://www.rfc-editor.org/rfc/rfc6749>
- T. Lodderstedt, J. Bradley, A. Labunets, and D. Fett, "Best Current Practice for OAuth 2.0
  Security," RFC 9700, IETF, Jan. 2025. <https://www.rfc-editor.org/rfc/rfc9700>
- W3C, "Content Security Policy Level 3," W3C Working Draft. <https://www.w3.org/TR/CSP3/>
- W3C, "Trusted Types," W3C Working Draft. <https://www.w3.org/TR/trusted-types/>
- W3C, "Web Content Accessibility Guidelines (WCAG) 2.2," W3C Recommendation, Oct. 2023.
  <https://www.w3.org/TR/WCAG22/>

[rfc9110]: https://www.rfc-editor.org/rfc/rfc9110
[rfc6749]: https://www.rfc-editor.org/rfc/rfc6749
[rfc9700]: https://www.rfc-editor.org/rfc/rfc9700
[csp]: https://www.w3.org/TR/CSP3/
[tt]: https://www.w3.org/TR/trusted-types/
[wcag]: https://www.w3.org/TR/WCAG22/
[zds]: https://github.com/zeabur/design-system
[shadcn]: https://ui.shadcn.com
[geist]: https://github.com/vercel/geist-font
