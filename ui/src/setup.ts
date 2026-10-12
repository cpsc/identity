// Setup page: registers a passkey with the one-time token from the URL fragment.
import { cancelled, creationOptions, post, problem, registrationJson, say, supported, upstreamButtons } from "./webauthn";

const form = document.getElementById("setup") as HTMLFormElement | null;
const messages = form?.dataset ?? {};
const token = new URLSearchParams(window.location.hash.slice(1)).get("token");

// Keep the token when switching language: the links carry the fragment.
for (const link of document.querySelectorAll<HTMLAnchorElement>("a[data-lang-link]")) {
  link.hash = window.location.hash;
}

async function register(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  if (!token) return;
  say("error", "");
  say("status", messages.msgWorking ?? "");
  try {
    const started = await post("/setup/options", { token });
    if (!started.ok) {
      say("status", "");
      say("error", problem(started) === "setup-invalid" ? messages.msgInvalid ?? "" : messages.msgError ?? "");
      return;
    }
    const credential = await navigator.credentials.create({ publicKey: creationOptions(started.body) });
    if (!(credential instanceof PublicKeyCredential)) throw new DOMException("no credential", "NotAllowedError");
    const label = (document.getElementById("label") as HTMLInputElement | null)?.value.trim();
    const finished = await post("/setup", { token, credential: registrationJson(credential), label: label || null });
    say("status", "");
    if (!finished.ok) {
      const kind = problem(finished);
      say("error", kind === "setup-invalid" ? messages.msgInvalid ?? "" : kind === "passkey-failed" ? messages.msgFailed ?? "" : messages.msgError ?? "");
      return;
    }
    // Done: forget the token in the address bar and history, show the result.
    window.history.replaceState(null, "", window.location.pathname + window.location.search);
    if (form) form.hidden = true;
    const done = document.getElementById("done");
    done?.removeAttribute("hidden");
    done?.focus();
  } catch (error) {
    say("status", "");
    say("error", cancelled(error) ? messages.msgCancelled ?? "" : messages.msgFailed ?? "");
  }
}

if (form) {
  // Linking an upstream account works without passkey support.
  if (token) upstreamButtons(() => ({ token }), messages);
  else document.getElementById("upstream")?.setAttribute("hidden", "");
  if (!supported()) {
    document.getElementById("unsupported")?.removeAttribute("hidden");
    form.hidden = true;
  } else if (!token) {
    say("error", messages.msgMissing ?? "");
    form.hidden = true;
  } else {
    form.addEventListener("submit", (event) => void register(event));
  }
}
