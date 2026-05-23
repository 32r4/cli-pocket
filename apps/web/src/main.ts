import "@terminal/styles/app.css";
import "./styles/web.css";

async function boot() {
  const root = document.getElementById("root")!;
  root.textContent = "loading…";
  const { startWebApp } = await import("./pairing/PairingFlow");
  await startWebApp(root);
}

boot().catch((e) => {
  console.error(e);
  document.body.innerHTML = `<pre style="color:#f88;padding:2rem">${String(e)}</pre>`;
});
