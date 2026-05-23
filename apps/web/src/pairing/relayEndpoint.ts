export interface ServerSelector {
  daemon_pairing_url: string; // e.g. ws://192.168.1.10:9443  (direct daemon pairing socket)
  code: string; // 6-digit code displayed by daemon at pair time
}

export interface SavedServer {
  endpoint_url: string; // post-pairing connect URL (same as daemon_pairing_url for v1)
  server_public_hex: string; // 64 hex chars
  client_public_hex: string;
  resume_token_hex?: string | null;
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
