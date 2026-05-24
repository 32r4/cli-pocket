export interface ServerSelector {
  daemon_url: string; // e.g. ws://127.0.0.1:7842
  code: string; // 6-digit code displayed by daemon at pair time
}

export interface SavedServer {
  endpoint_url: string; // post-pairing connect URL, derived as /session
  server_public_hex: string; // 64 hex chars
  client_public_hex: string;
  resume_token_hex?: string | null;
}

export interface DaemonEndpoints {
  pairing_url: string;
  session_url: string;
}

export function deriveDaemonEndpoints(daemonUrl: string): DaemonEndpoints {
  const url = new URL(daemonUrl);
  url.pathname = "/pair";
  url.search = "";
  url.hash = "";
  const pairingUrl = url.toString();

  url.pathname = "/session";
  return {
    pairing_url: pairingUrl,
    session_url: url.toString(),
  };
}

export function validateUrl(s: string): string | null {
  if (!/^wss?:\/\//.test(s)) return "URL must start with ws:// or wss://";
  try {
    new URL(s);
    return null;
  } catch {
    return "invalid URL";
  }
}

export function validateCode(s: string): string | null {
  if (!/^\d{6}$/.test(s)) return "code must be 6 digits";
  return null;
}
