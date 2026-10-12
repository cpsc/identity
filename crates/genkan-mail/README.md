# genkan-mail

A minimal SMTP submission client ([RFC 6409][rfc6409]) that sends invitation links: one
plain-text UTF-8 message to one recipient per call. The entry point is `Mailer`, built from an
`smtps://` or `smtp://` URL with `Mailer::new` and used with `Mailer::send`.

Depends only on `genkan-core` (clock and randomness); TLS comes from rustls with caller-supplied
roots. Used by the `genkan` server, which mails pending invitations when an SMTP server is
configured.

## Security properties

- **TLS is mandatory.** `smtps://` uses implicit TLS from the first byte (port 465, as
  [RFC 8314 §3.3][rfc8314] recommends for submission). `smtp://` (port 587) must upgrade with
  STARTTLS ([RFC 3207][rfc3207]) and the session fails if the server does not offer it; there is
  no cleartext fallback.
- **STARTTLS injection guard.** Any bytes the server sent after its `220` reply to STARTTLS and
  before the handshake could be injected by an attacker on the path (the command-injection class
  discussed in [RFC 3207 §6][rfc3207]); the session is dropped. After the handshake everything
  learnt before TLS is discarded and `EHLO` is sent again ([RFC 3207 §4.2][rfc3207]).
- **Credentials only inside TLS,** with `AUTH PLAIN` ([RFC 4616][rfc4616]).
- **No header or command injection.** Addresses must be plain ASCII `local@domain` (stricter
  than RFC 5321 on purpose); the subject is an encoded word and the body is base64, so no input
  can start a new protocol line or header.
- **Bounded.** Reply lines are limited to the 512 octets of [RFC 5321 §4.5.3.1.5][rfc5321] and
  the whole conversation has a timeout.

## References

- R. Gellens and J. Klensin, "Message Submission for Mail," RFC 6409, IETF, Nov. 2011.
  <https://www.rfc-editor.org/rfc/rfc6409>
- K. Moore and C. Newman, "Cleartext Considered Obsolete: Use of Transport Layer Security (TLS)
  for Email Submission and Access," RFC 8314, IETF, Jan. 2018.
  <https://www.rfc-editor.org/rfc/rfc8314>
- P. Hoffman, "SMTP Service Extension for Secure SMTP over Transport Layer Security,"
  RFC 3207, IETF, Feb. 2002. <https://www.rfc-editor.org/rfc/rfc3207>
- K. Zeilenga, Ed., "The PLAIN Simple Authentication and Security Layer (SASL) Mechanism,"
  RFC 4616, IETF, Aug. 2006. <https://www.rfc-editor.org/rfc/rfc4616>
- J. Klensin, "Simple Mail Transfer Protocol," RFC 5321, IETF, Oct. 2008.
  <https://www.rfc-editor.org/rfc/rfc5321>

[rfc6409]: https://www.rfc-editor.org/rfc/rfc6409
[rfc8314]: https://www.rfc-editor.org/rfc/rfc8314
[rfc3207]: https://www.rfc-editor.org/rfc/rfc3207
[rfc4616]: https://www.rfc-editor.org/rfc/rfc4616
[rfc5321]: https://www.rfc-editor.org/rfc/rfc5321
