import "@terminal/styles/app.css";
import "./styles/web.css";

async function boot() {
  const root = document.getElementById("root")!;
  root.textContent = "loading…";
  // PairingFlow lands in Task I7. For the skeleton, show a placeholder.
  root.textContent = "cli-pocket web app — pairing UI lands in I7";
}

boot().catch((e) => {
  console.error(e);
  document.body.innerHTML = `<pre style="color:#f88;padding:2rem">${String(e)}</pre>`;
});
