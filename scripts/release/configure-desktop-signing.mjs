import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const configPath = resolve("apps/desktop/src-tauri/tauri.conf.json");
const config = JSON.parse(readFileSync(configPath, "utf8"));

const thumbprint = process.env.WINDOWS_CERTIFICATE_THUMBPRINT;
const signCommand = process.env.WINDOWS_SIGN_COMMAND;

if (!thumbprint && !signCommand) {
  console.log("Windows signing is not configured.");
  process.exit(0);
}

const bundle = (config.bundle ??= {});
const windows = (bundle.windows ??= {});

if (thumbprint) {
  windows.certificateThumbprint = thumbprint;
  windows.digestAlgorithm = process.env.WINDOWS_DIGEST_ALGORITHM || "sha256";
  windows.timestampUrl =
    process.env.WINDOWS_TIMESTAMP_URL || "http://timestamp.digicert.com";
}

if (signCommand) {
  windows.signCommand = signCommand;
}

writeFileSync(configPath, `${JSON.stringify(config, null, 2)}\n`, "utf8");
console.log(`Configured desktop signing in ${configPath}`);
