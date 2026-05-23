// IdentityActions.ts — export/import identity via file download / file picker.
//
// Works through a minimal IdentityClient interface so this module is not
// coupled to the wasm layer directly.  WebBridge satisfies this interface.
//
// Hash-based triggers:
//   #export  — downloads identity as a .txt file, then clears the hash
//   #import  — opens a file picker, imports the identity, reloads the page

export interface IdentityClient {
  exportIdentity(): Promise<Uint8Array>;
  importIdentity(blob: Uint8Array): Promise<void>;
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

export async function exportIdentityToFile(
  client: IdentityClient,
): Promise<void> {
  const bytes = await client.exportIdentity();
  const blob = new Blob([bytes], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `cli-pocket-identity-${new Date().toISOString().slice(0, 10)}.txt`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  URL.revokeObjectURL(url);
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

export async function importIdentityFromPicker(
  client: IdentityClient,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".txt,application/octet-stream";
    input.onchange = async () => {
      try {
        const f = input.files?.[0];
        if (!f) {
          resolve();
          return;
        }
        const buf = await f.arrayBuffer();
        await client.importIdentity(new Uint8Array(buf));
        resolve();
      } catch (e) {
        reject(e);
      }
    };
    input.click();
  });
}

// ---------------------------------------------------------------------------
// Hash-based dispatch
// ---------------------------------------------------------------------------

export function installHashHandlers(client: IdentityClient): void {
  const handler = async () => {
    if (location.hash === "#export") {
      history.replaceState(null, "", location.pathname);
      try {
        await exportIdentityToFile(client);
      } catch (e) {
        alert(`export failed: ${e}`);
      }
    } else if (location.hash === "#import") {
      history.replaceState(null, "", location.pathname);
      try {
        await importIdentityFromPicker(client);
        location.reload();
      } catch (e) {
        alert(`import failed: ${e}`);
      }
    }
  };

  window.addEventListener("hashchange", handler);
  // Handle hash already present in URL on page load.
  handler();
}
