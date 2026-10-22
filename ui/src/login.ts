// Sign-in page: passkey autofill (conditional mediation) plus an explicit button.
import { assertionJson, cancelled, post, problem, requestOptions, say, signal, supported, upstreamButtons } from "./webauthn";

const form = document.getElementById("sign-in") as HTMLFormElement | null;
const messages = form?.dataset ?? {};
let autofill: AbortController | null = null;
const usernameField = document.getElementById("username-field");
const useUsername = document.getElementById("use-username");

// Without a login hint the field is only where the browser lists passkeys, so it stays folded
// behind "Use a username" while the browser can offer them there.
function fold(folded: boolean): void {
  if (!usernameField?.hasAttribute("data-collapsible") || !useUsername) return;
  usernameField.hidden = folded;
  useUsername.hidden = !folded;
}

async function finish(credential: Credential | null): Promise<void> {
  if (!(credential instanceof PublicKeyCredential)) throw new DOMException("no credential", "NotAllowedError");
  const reply = await post("/login", { credential: assertionJson(credential) });
  await signal(reply.body);
  if (reply.ok && typeof reply.body.redirect_to === "string") {
    say("status", "");
    window.location.assign(reply.body.redirect_to);
    return;
  }
  say("status", "");
  say("error", problem(reply) === "passkey-failed" ? messages.msgFailed ?? "" : messages.msgError ?? "");
}

async function options(): Promise<PublicKeyCredentialRequestOptions | null> {
  const reply = await post("/login/options", {});
  if (!reply.ok) {
    say("error", messages.msgError ?? "");
    return null;
  }
  return requestOptions(reply.body);
}

async function startAutofill(): Promise<void> {
  const available = await PublicKeyCredential.isConditionalMediationAvailable?.();
  if (!available) {
    fold(false);
    return;
  }
  const publicKey = await options();
  if (!publicKey) return;
  autofill = new AbortController();
  try {
    const credential = await navigator.credentials.get({ publicKey, mediation: "conditional", signal: autofill.signal });
    await finish(credential);
  } catch (error) {
    // Aborted when the user presses the button instead; other errors surface on the next attempt.
    if (!cancelled(error)) say("error", messages.msgError ?? "");
  }
}

async function signIn(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  autofill?.abort();
  say("error", "");
  say("status", messages.msgWorking ?? "");
  try {
    const publicKey = await options();
    if (!publicKey) return;
    await finish(await navigator.credentials.get({ publicKey }));
  } catch (error) {
    say("status", "");
    say("error", cancelled(error) ? messages.msgCancelled ?? "" : messages.msgFailed ?? "");
  }
}

if (form) {
  // Upstream providers work without passkey support.
  upstreamButtons(() => {
    autofill?.abort();
    return {};
  }, messages);
  if (!supported()) {
    document.getElementById("unsupported")?.removeAttribute("hidden");
    form.hidden = true;
  } else {
    form.addEventListener("submit", (event) => void signIn(event));
    useUsername?.addEventListener("click", () => {
      fold(false);
      document.getElementById("username")?.focus();
    });
    fold(typeof PublicKeyCredential.isConditionalMediationAvailable === "function");
    void startAutofill();
  }
}
