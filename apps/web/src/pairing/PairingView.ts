import { validateCode, validateUrl } from "./relayEndpoint";

export interface PairingValues {
  daemon_pairing_url: string;
  code: string;
}

export function mountPairingView(
  parent: HTMLElement,
  onSubmit: (v: PairingValues) => Promise<void>,
): void {
  parent.innerHTML = `
    <form class="web-pair-card" novalidate>
      <h1 style="margin:0 0 0.5rem 0;font-size:1.1rem">Pair with a host</h1>
      <label>Daemon pairing URL
        <input name="url" placeholder="ws://192.168.1.10:9443" required>
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
    const v: PairingValues = {
      daemon_pairing_url: String(fd.get("url") ?? "").trim(),
      code: String(fd.get("code") ?? "").trim(),
    };
    const errs = [validateUrl(v.daemon_pairing_url), validateCode(v.code)].filter(
      Boolean,
    ) as string[];
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
