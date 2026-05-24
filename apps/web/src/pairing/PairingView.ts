import { deriveDaemonEndpoints, validateCode, validateUrl } from "./relayEndpoint";

export interface PairingValues {
  pairing_url: string;
  session_url: string;
  code: string;
}

export function mountPairingView(
  parent: HTMLElement,
  onSubmit: (v: PairingValues) => Promise<void>,
): void {
  parent.innerHTML = `
    <form class="web-pair-card" novalidate>
      <h1 style="margin:0 0 0.5rem 0;font-size:1.1rem">Pair with a host</h1>
      <label>Daemon URL
        <input name="daemon_url" value="ws://127.0.0.1:7842" placeholder="ws://127.0.0.1:7842" required>
      </label>
      <label>6-digit code
        <input name="code" placeholder="000000" inputmode="numeric" pattern="[0-9]{6}" required>
      </label>
      <button type="submit">Pair</button>
      <div class="err" data-role="err" hidden></div>
    </form>
  `;
  const form = parent.querySelector("form")!;
  const errEl = form.querySelector<HTMLElement>('[data-role="err"]')!;

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const fd = new FormData(form);
    const daemonUrl = String(fd.get("daemon_url") ?? "").trim();
    const endpoints = validateUrl(daemonUrl) ? null : deriveDaemonEndpoints(daemonUrl);
    const v: PairingValues = {
      pairing_url: endpoints?.pairing_url ?? "",
      session_url: endpoints?.session_url ?? "",
      code: String(fd.get("code") ?? "").trim(),
    };
    const errs = [
      validateUrl(daemonUrl),
      validateCode(v.code),
    ].filter(Boolean) as string[];
    if (errs.length) {
      errEl.textContent = errs.join(" · ");
      errEl.hidden = false;
      return;
    }
    errEl.hidden = true;
    const btn = form.querySelector("button") as HTMLButtonElement;
    btn.disabled = true;
    try {
      await onSubmit(v);
    } catch (err) {
      errEl.textContent = String(err);
      errEl.hidden = false;
      btn.disabled = false;
    }
  });
}
