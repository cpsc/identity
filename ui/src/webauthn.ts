// Shared helpers for the sign-in pages: WebAuthn JSON conversion (webauthn-rs wire format), JSON
// requests, and accessible status messages. No HTML is ever written (Trusted Types: 'none').

type Json = Record<string, unknown>;

export function toBase64url(buffer: ArrayBuffer | ArrayBufferView): string {
  const bytes = buffer instanceof ArrayBuffer ? new Uint8Array(buffer) : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64url(value: string): ArrayBuffer {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes.buffer;
}

type Descriptor = { id: string; type: string; transports?: string[] };

function descriptors(list: unknown): PublicKeyCredentialDescriptor[] | undefined {
  if (!Array.isArray(list)) return undefined;
  return (list as Descriptor[]).map((d) => ({ ...d, id: fromBase64url(d.id) }) as PublicKeyCredentialDescriptor);
}

/** `CreationChallengeResponse` JSON → options for `navigator.credentials.create`. */
export function creationOptions(json: Json): PublicKeyCredentialCreationOptions {
  const o = json.publicKey as Json;
  const user = o.user as Json;
  return {
    ...(o as unknown as PublicKeyCredentialCreationOptions),
    challenge: fromBase64url(o.challenge as string),
    user: { ...(user as unknown as PublicKeyCredentialUserEntity), id: fromBase64url(user.id as string) },
    excludeCredentials: descriptors(o.excludeCredentials),
  };
}

/** `RequestChallengeResponse` JSON → options for `navigator.credentials.get`. */
export function requestOptions(json: Json): PublicKeyCredentialRequestOptions {
  const o = json.publicKey as Json;
  return {
    ...(o as unknown as PublicKeyCredentialRequestOptions),
    challenge: fromBase64url(o.challenge as string),
    allowCredentials: descriptors(o.allowCredentials) ?? [],
  };
}

/** A new credential → webauthn-rs `RegisterPublicKeyCredential`. */
export function registrationJson(credential: PublicKeyCredential): Json {
  const response = credential.response as AuthenticatorAttestationResponse;
  return {
    id: credential.id,
    rawId: toBase64url(credential.rawId),
    type: credential.type,
    response: {
      attestationObject: toBase64url(response.attestationObject),
      clientDataJSON: toBase64url(response.clientDataJSON),
      transports: typeof response.getTransports === "function" ? response.getTransports() : undefined,
    },
    extensions: credential.getClientExtensionResults(),
  };
}

/** An assertion → webauthn-rs `PublicKeyCredential`. */
export function assertionJson(credential: PublicKeyCredential): Json {
  const response = credential.response as AuthenticatorAssertionResponse;
  return {
    id: credential.id,
    rawId: toBase64url(credential.rawId),
    type: credential.type,
    response: {
      authenticatorData: toBase64url(response.authenticatorData),
      clientDataJSON: toBase64url(response.clientDataJSON),
      signature: toBase64url(response.signature),
      userHandle: response.userHandle ? toBase64url(response.userHandle) : null,
    },
    extensions: credential.getClientExtensionResults(),
  };
}

export type Reply = { ok: boolean; status: number; body: Json };

/** POSTs JSON to the issuer. Same-origin only; the server also checks Fetch Metadata. */
export async function post(path: string, body: Json): Promise<Reply> {
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  let json: Json = {};
  try {
    json = (await response.json()) as Json;
  } catch {
    // Empty or non-JSON body.
  }
  return { ok: response.ok, status: response.status, body: json };
}

/** The problem type suffix from an RFC 9457 response, for example "setup-invalid". */
export function problem(reply: Reply): string {
  const type = typeof reply.body.type === "string" ? reply.body.type : "";
  return type.replace("urn:zeabur-identity:problem:", "");
}

export function supported(): boolean {
  return typeof window.PublicKeyCredential === "function" && typeof navigator.credentials?.create === "function";
}

/** Writes `text` into a live region (textContent only). */
export function say(id: "status" | "error", text: string): void {
  const element = document.getElementById(id);
  if (element) element.textContent = text;
}

/**
 * Wires the "Continue with <provider>" buttons: each starts an upstream sign-in (or, with a
 * token, linking) and follows the provider URL the server returns.
 */
export function upstreamButtons(body: () => Json, messages: DOMStringMap): void {
  for (const button of document.querySelectorAll<HTMLButtonElement>("button[data-provider]")) {
    button.addEventListener("click", () => {
      void (async () => {
        say("error", "");
        say("status", messages.msgWorking ?? "");
        try {
          const reply = await post(`/upstream/${button.dataset.provider ?? ""}/start`, body());
          if (reply.ok && typeof reply.body.redirect_to === "string") {
            window.location.assign(reply.body.redirect_to);
            return;
          }
          say("status", "");
          say("error", problem(reply) === "setup-invalid" ? messages.msgInvalid ?? "" : messages.msgError ?? "");
        } catch {
          say("status", "");
          say("error", messages.msgError ?? "");
        }
      })();
    });
  }
}

/** Whether an error is the user cancelling or the browser's timeout. */
export function cancelled(error: unknown): boolean {
  return error instanceof DOMException && (error.name === "NotAllowedError" || error.name === "AbortError");
}
