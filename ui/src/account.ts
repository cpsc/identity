// Account page: sign in, then rename, add and remove passkeys and end sessions. Adding and
// removing need a passkey confirmation from the last five minutes; the server says when one is
// due ("reauth-required") and the page asks for it, then retries once.
import {
  assertionJson,
  cancelled,
  creationOptions,
  post,
  problem,
  type Reply,
  registrationJson,
  requestOptions,
  signal,
  supported,
} from "./webauthn"

const root = document.getElementById("account")
const messages = root?.dataset ?? {}
const csrf = messages.csrf ?? ""

const errors: Record<string, string | undefined> = {
  "passkey-failed": messages.msgFailed,
  "passkey-exists": messages.msgExists,
  "last-passkey": messages.msgLast,
  conflict: messages.msgChanged,
  "not-found": messages.msgGone,
  "signed-out": messages.msgSignedOut,
}

/** Where a task reports progress and failure: the page header, or a passkey's row. */
type Place = { status: HTMLElement | null; error: HTMLElement | null }

const header: Place = {
  status: document.getElementById("status"),
  error: document.getElementById("error"),
}

/** Problems that mean the page shows a stale list: reload it from the server, then explain. */
const stale = new Set(["last-passkey", "conflict", "not-found"])

function write(element: HTMLElement | null, text: string): void {
  if (element) element.textContent = text
}

function fail(place: Place, reply: Reply | null, error?: unknown): void {
  write(place.status, "")
  if (error !== undefined)
    write(
      place.error,
      cancelled(error) ? (messages.msgCancelled ?? "") : (messages.msgFailed ?? ""),
    )
  else write(place.error, (reply && errors[problem(reply)]) ?? messages.msgError ?? "")
}

function refresh(code: string, passkey: string): void {
  if (code === "not-found" && !passkey) return window.location.reload()
  const notice = new URLSearchParams({ notice: code })
  if (passkey) notice.set("passkey", passkey)
  window.location.hash = notice.toString()
  window.location.reload()
}

/** After `refresh`: shows its message on the passkey's row if it is still listed, else at the top. */
function showNotice(): void {
  const notice = new URLSearchParams(window.location.hash.slice(1))
  const code = notice.get("notice")
  if (!code || !stale.has(code)) return
  window.history.replaceState(null, "", window.location.pathname + window.location.search)
  const name = notice.get("passkey")
  const row = [...document.querySelectorAll<HTMLElement>("[data-passkey]")].find(
    (item) => item.dataset.passkey === name,
  )
  const place = row ? rowPlace(row) : header
  write(place.error, errors[code] ?? messages.msgError ?? "")
}

function rowPlace(item: HTMLElement): Place {
  return { status: item.querySelector("[data-status]"), error: item.querySelector("[data-error]") }
}

async function assertion(
  path: string,
  extra: Record<string, unknown>,
): Promise<PublicKeyCredential> {
  const started = await post(path, extra)
  if (!started.ok) throw started
  const credential = await navigator.credentials.get({ publicKey: requestOptions(started.body) })
  if (!(credential instanceof PublicKeyCredential))
    throw new DOMException("no credential", "NotAllowedError")
  return credential
}

async function reauthenticate(place: Place): Promise<Reply> {
  write(place.status, messages.msgConfirm ?? "")
  const credential = await assertion("/account/reauth/options", { csrf })
  return post("/account/reauth", { csrf, credential: assertionJson(credential) })
}

/** Runs `action`; if the server wants a fresh confirmation, gets one and runs it again. */
async function fresh(place: Place, action: () => Promise<Reply>): Promise<Reply> {
  const first = await action()
  if (problem(first) !== "reauth-required") return first
  const confirmed = await reauthenticate(place)
  return confirmed.ok ? action() : confirmed
}

/**
 * Wraps a click or submit handler: status while working, then reload or explain at `place`.
 * `passkey` names the row's passkey, so a stale-list refusal can point back at it.
 */
function run(task: () => Promise<Reply | null>, place: Place = header, passkey = ""): void {
  for (const p of new Set([header, place])) write(p.error, "")
  write(place.status, messages.msgWorking ?? "")
  const failed = (reply: Reply): void => {
    const code = problem(reply)
    if (stale.has(code)) refresh(code, passkey)
    else fail(place, reply)
  }
  task()
    .then(async (reply) => {
      if (!reply) return
      if (!reply.ok) return failed(reply)
      await signal(reply.body)
      window.location.reload()
    })
    .catch((error: unknown) => {
      if (error && typeof error === "object" && "status" in error) failed(error as Reply)
      else fail(place, null, error)
    })
}

function signIn(): void {
  run(async () => {
    const credential = await assertion("/account/session/options", {})
    return post("/account/session", { credential: assertionJson(credential) })
  })
}

function addPasskey(event: SubmitEvent): void {
  event.preventDefault()
  const label = (document.getElementById("add-label") as HTMLInputElement | null)?.value.trim()
  run(async () => {
    const started = await fresh(header, () => post("/account/passkeys/options", { csrf }))
    if (!started.ok) return started
    write(header.status, messages.msgWorking ?? "")
    const credential = await navigator.credentials.create({
      publicKey: creationOptions(started.body),
    })
    if (!(credential instanceof PublicKeyCredential))
      throw new DOMException("no credential", "NotAllowedError")
    return post("/account/passkeys", {
      csrf,
      credential: registrationJson(credential),
      label: label || null,
    })
  })
}

function show(item: HTMLElement, part: "rename" | "confirm" | null): void {
  const actions = item.querySelector<HTMLElement>("[data-actions]")
  const rename = item.querySelector<HTMLElement>("[data-rename]")
  const confirm = item.querySelector<HTMLElement>("[data-confirm]")
  if (actions) actions.hidden = part !== null
  if (rename) rename.hidden = part !== "rename"
  if (confirm) confirm.hidden = part !== "confirm"
  if (part === "rename") rename?.querySelector("input")?.focus()
  if (part === "confirm")
    confirm?.querySelector<HTMLElement>("[data-action=confirm-remove]")?.focus()
}

function wirePasskey(item: HTMLElement): void {
  const name = item.dataset.passkey ?? ""
  item.querySelector("[data-action=rename]")?.addEventListener("click", () => show(item, "rename"))
  item.querySelector("[data-action=remove]")?.addEventListener("click", () => show(item, "confirm"))
  for (const cancel of item.querySelectorAll("[data-action=cancel]")) {
    cancel.addEventListener("click", () => {
      write(rowPlace(item).error, "")
      show(item, null)
    })
  }
  item.querySelector<HTMLFormElement>("[data-rename]")?.addEventListener("submit", (event) => {
    event.preventDefault()
    const label = item.querySelector<HTMLInputElement>("[data-rename] input")?.value.trim() ?? ""
    if (label)
      run(() => post("/account/passkeys/rename", { csrf, name, label }), rowPlace(item), name)
  })
  item.querySelector("[data-action=confirm-remove]")?.addEventListener("click", () => {
    const place = rowPlace(item)
    run(() => fresh(place, () => post("/account/passkeys/remove", { csrf, name })), place, name)
  })
}

function localizeDates(): void {
  const format = new Intl.DateTimeFormat(messages.lang, { dateStyle: "medium" })
  for (const time of document.querySelectorAll<HTMLTimeElement>("time[data-format]")) {
    const date = new Date(time.dateTime)
    if (!Number.isNaN(date.getTime()))
      time.textContent = (time.dataset.format ?? "%date%").replace("%date%", format.format(date))
  }
}

if (root) {
  localizeDates()
  document
    .getElementById("sign-out")
    ?.addEventListener("click", () => run(() => post("/account/sign-out", { csrf })))
  document.getElementById("sign-out-others")?.addEventListener("click", () => {
    run(() => post("/account/sessions/revoke-others", { csrf }))
  })
  for (const item of document.querySelectorAll<HTMLElement>("[data-session]")) {
    item.querySelector("[data-action=revoke]")?.addEventListener("click", () => {
      run(() => post("/account/sessions/revoke", { csrf, name: item.dataset.session ?? "" }))
    })
  }
  const signInButton = document.getElementById("account-sign-in")
  const add = document.getElementById("add") as HTMLFormElement | null
  if (!supported()) {
    document.getElementById("unsupported")?.removeAttribute("hidden")
    if (signInButton) signInButton.hidden = true
    if (add) add.hidden = true
  } else {
    signInButton?.addEventListener("click", signIn)
    add?.addEventListener("submit", addPasskey)
    for (const item of document.querySelectorAll<HTMLElement>("[data-passkey]")) wirePasskey(item)
    if (window.location.hash === "#add") document.getElementById("add-label")?.focus()
  }
  showNotice()
}
