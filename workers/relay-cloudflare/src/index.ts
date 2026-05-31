interface Env {
  RELAY: DurableObjectNamespace;
  SERVER_AUTH_TOKEN?: string;
  MAX_SERVERS?: string;
  MAX_PAIRS?: string;
  MAX_BYTES_PER_SEC?: string;
  MAX_QUEUED_BYTES?: string;
  IDLE_SECONDS?: string;
}

interface DurableObjectNamespace {
  idFromName(name: string): DurableObjectId;
  get(id: DurableObjectId): DurableObjectStub;
}

interface DurableObjectId {
  toString(): string;
}

interface DurableObjectStub {
  fetch(request: Request): Promise<Response>;
}

interface DurableObjectState {
  acceptWebSocket(ws: WebSocket, tags?: string[]): void;
  getWebSockets(tag?: string): WebSocket[];
  storage: DurableObjectStorage;
  blockConcurrencyWhile<T>(callback: () => Promise<T>): Promise<T>;
}

interface DurableObjectStorage {
  get<T>(key: string): Promise<T | undefined>;
  put<T>(key: string, value: T): Promise<void>;
  delete(key: string): Promise<boolean>;
}

interface RelayWebSocket extends WebSocket {
  serializeAttachment(value: unknown): void;
  deserializeAttachment(): unknown;
}

type CloseReason =
  | { type: "Normal" }
  | { type: "ServerGone" }
  | { type: "ClientGone" }
  | { type: "Stuck" }
  | { type: "RelayShutdown" }
  | { type: "Rejected"; message: string };

type RelayMessage =
  | { kind: "ServerRegister"; serverId: string }
  | { kind: "ServerRegisterOk" }
  | { kind: "ServerRegisterErr"; reason: string }
  | { kind: "ServerHeartbeat" }
  | { kind: "ClientConnect"; serverId: string }
  | { kind: "PairInbound"; pairId: string }
  | { kind: "PairOpen"; pairId: string }
  | { kind: "PairClose"; pairId: string; reason: CloseReason }
  | { kind: "DataForward"; pairId: string; bytes: Uint8Array };

type SocketAttachment =
  | {
      socketRole: "server";
      registered: boolean;
      serverId: string | null;
      connectedAt: number;
      lastProgressAt: number;
    }
  | {
      socketRole: "client";
      targetServerId: string;
      pairId: string | null;
      connectedAt: number;
      lastProgressAt: number;
    };

interface PairState {
  pairId: string;
  serverId: string;
  createdAt: number;
  lastProgressAt: number;
  queuedBytes: number;
  bytesThisWindow: number;
  windowStartedAt: number;
}

interface RelayLimits {
  maxServers: number;
  maxPairs: number;
  maxBytesPerSec: number;
  maxQueuedBytes: number;
  idleSeconds: number;
}

const RELAY_DISC_CTRL = 0x01;
const RELAY_DISC_DATA = 0x02;

const CTRL_SERVER_REGISTER = 0x00;
const CTRL_SERVER_REGISTER_OK = 0x01;
const CTRL_SERVER_REGISTER_ERR = 0x02;
const CTRL_SERVER_HEARTBEAT = 0x03;
const CTRL_CLIENT_CONNECT = 0x04;
const CTRL_PAIR_INBOUND = 0x05;
const CTRL_PAIR_OPEN = 0x06;
const CTRL_PAIR_CLOSE = 0x07;

function json(data: unknown, status = 200): Response {
  return new Response(JSON.stringify(data), {
    status,
    headers: {
      "content-type": "application/json; charset=utf-8",
    },
  });
}

function requireWebSocketUpgrade(request: Request): Response | null {
  if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
    return new Response("Expected WebSocket upgrade", { status: 426 });
  }
  return null;
}

function getWebSocketPair(): [WebSocket, WebSocket] {
  const pairCtor = Reflect.get(globalThis, "WebSocketPair") as
    | (new () => { 0: WebSocket; 1: WebSocket })
    | undefined;
  if (!pairCtor) {
    throw new Error("WebSocketPair is not available in this runtime");
  }
  const pair = new pairCtor();
  return [pair[0], pair[1]];
}

function responseWithWebSocket(webSocket: WebSocket): Response {
  return new Response(null, {
    status: 101,
    webSocket,
  } as ResponseInit & { webSocket: WebSocket });
}

function isUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(
    value,
  );
}

function parsePositiveInt(raw: string | undefined, fallback: number): number {
  const parsed = Number.parseInt(raw ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback;
}

function readLimits(env: Env): RelayLimits {
  return {
    maxServers: parsePositiveInt(env.MAX_SERVERS, 256),
    maxPairs: parsePositiveInt(env.MAX_PAIRS, 2048),
    maxBytesPerSec: parsePositiveInt(env.MAX_BYTES_PER_SEC, 4 * 1024 * 1024),
    maxQueuedBytes: parsePositiveInt(env.MAX_QUEUED_BYTES, 8 * 1024 * 1024),
    idleSeconds: parsePositiveInt(env.IDLE_SECONDS, 120),
  };
}

function getClientTargetServerId(url: URL): string | null {
  const value = url.searchParams.get("server");
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  return isUuid(trimmed) ? trimmed : null;
}

function getServerIdFromUrl(url: URL): string | null {
  const value = url.searchParams.get("server");
  if (!value) return null;
  const trimmed = value.trim().toLowerCase();
  return isUuid(trimmed) ? trimmed : null;
}

function parseSocketRole(pathname: string): SocketAttachment["socketRole"] | null {
  if (pathname === "/ws/server") {
    return "server";
  }
  if (pathname === "/ws/client") {
    return "client";
  }
  return null;
}

function initialAttachment(request: Request): SocketAttachment | null {
  const url = new URL(request.url);
  const socketRole = parseSocketRole(url.pathname);
  if (!socketRole) {
    return null;
  }
  if (socketRole === "server") {
    return {
      socketRole,
      registered: false,
      serverId: getServerIdFromUrl(url),
      connectedAt: Date.now(),
      lastProgressAt: Date.now(),
    };
  }
  const targetServerId = getClientTargetServerId(url);
  if (!targetServerId) {
    return null;
  }
  return {
    socketRole,
    targetServerId,
    pairId: null,
    connectedAt: Date.now(),
    lastProgressAt: Date.now(),
  };
}

function asBytes(message: string | ArrayBuffer): Uint8Array | null {
  if (typeof message === "string") {
    return null;
  }
  return new Uint8Array(message);
}

function bytesToUuid(bytes: Uint8Array): string {
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
}

function uuidToBytes(uuid: string): Uint8Array {
  const compact = uuid.replace(/-/g, "");
  const out = new Uint8Array(16);
  for (let index = 0; index < 16; index += 1) {
    out[index] = Number.parseInt(compact.slice(index * 2, index * 2 + 2), 16);
  }
  return out;
}

function encodeUuid(uuid: string): Uint8Array {
  return encodeByteArray(uuidToBytes(uuid));
}

function encodeVarint(value: number): Uint8Array {
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`invalid varint value ${value}`);
  }

  const bytes: number[] = [];
  let remaining = value;
  do {
    let next = remaining % 0x80;
    remaining = Math.floor(remaining / 0x80);
    if (remaining > 0) {
      next |= 0x80;
    }
    bytes.push(next);
  } while (remaining > 0);

  return Uint8Array.from(bytes);
}

function encodeByteArray(bytes: Uint8Array): Uint8Array {
  return concatBytes([encodeVarint(bytes.length), bytes]);
}

function encodeString(value: string): Uint8Array {
  return encodeByteArray(new TextEncoder().encode(value));
}

function concatBytes(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

class Cursor {
  private offset = 0;

  constructor(private readonly bytes: Uint8Array) {}

  readByte(): number {
    if (this.offset >= this.bytes.length) {
      throw new Error("unexpected end of relay frame");
    }
    const value = this.bytes[this.offset];
    this.offset += 1;
    return value;
  }

  readBytes(length: number): Uint8Array {
    if (this.offset + length > this.bytes.length) {
      throw new Error("unexpected end of relay frame");
    }
    const out = this.bytes.slice(this.offset, this.offset + length);
    this.offset += length;
    return out;
  }

  readVarint(): number {
    let value = 0;
    let shift = 0;
    while (true) {
      const byte = this.readByte();
      value += (byte & 0x7f) * 2 ** shift;
      if ((byte & 0x80) === 0) {
        return value;
      }
      shift += 7;
      if (shift > 49) {
        throw new Error("varint too large");
      }
    }
  }

  readByteArray(): Uint8Array {
    const length = this.readVarint();
    return this.readBytes(length);
  }

  readUuid(): string {
    const bytes = this.readByteArray();
    if (bytes.length !== 16) {
      throw new Error(`unexpected uuid length ${bytes.length}`);
    }
    return bytesToUuid(bytes);
  }

  readString(): string {
    return new TextDecoder().decode(this.readByteArray());
  }
}

function decodeRelay(bytes: Uint8Array): RelayMessage {
  const cursor = new Cursor(bytes);
  const discriminator = cursor.readByte();

  if (discriminator === RELAY_DISC_CTRL) {
    const kind = cursor.readVarint();
    switch (kind) {
      case CTRL_SERVER_REGISTER: {
        const serverId = cursor.readUuid();
        cursor.readByteArray();
        cursor.readByteArray();
        return { kind: "ServerRegister", serverId };
      }
      case CTRL_SERVER_REGISTER_OK:
        return { kind: "ServerRegisterOk" };
      case CTRL_SERVER_REGISTER_ERR: {
        const reason = cursor.readString();
        return { kind: "ServerRegisterErr", reason };
      }
      case CTRL_SERVER_HEARTBEAT:
        return { kind: "ServerHeartbeat" };
      case CTRL_CLIENT_CONNECT: {
        const serverId = cursor.readUuid();
        return { kind: "ClientConnect", serverId };
      }
      case CTRL_PAIR_INBOUND: {
        const pairId = cursor.readUuid();
        return { kind: "PairInbound", pairId };
      }
      case CTRL_PAIR_OPEN: {
        const pairId = cursor.readUuid();
        return { kind: "PairOpen", pairId };
      }
      case CTRL_PAIR_CLOSE: {
        const pairId = cursor.readUuid();
        const reasonTag = cursor.readVarint();
        const reason =
          reasonTag === 0
            ? { type: "Normal" as const }
            : reasonTag === 1
              ? { type: "ServerGone" as const }
              : reasonTag === 2
                ? { type: "ClientGone" as const }
                : reasonTag === 3
                  ? { type: "Stuck" as const }
                  : reasonTag === 4
                    ? { type: "RelayShutdown" as const }
                  : {
                        type: "Rejected" as const,
                        message: cursor.readString(),
                      };
        return { kind: "PairClose", pairId, reason };
      }
      default:
        throw new Error(`unknown relay control kind ${kind}`);
    }
  }

  if (discriminator === RELAY_DISC_DATA) {
    const kind = cursor.readVarint();
    if (kind !== 0) {
      throw new Error(`unknown relay data kind ${kind}`);
    }
    const pairId = cursor.readUuid();
    const payload = cursor.readByteArray();
    return { kind: "DataForward", pairId, bytes: payload };
  }

  throw new Error(`unknown relay discriminator ${discriminator}`);
}

function encodePairCloseReason(reason: CloseReason): Uint8Array {
  switch (reason.type) {
    case "Normal":
      return encodeVarint(0);
    case "ServerGone":
      return encodeVarint(1);
    case "ClientGone":
      return encodeVarint(2);
    case "Stuck":
      return encodeVarint(3);
    case "RelayShutdown":
      return encodeVarint(4);
    case "Rejected": {
      return concatBytes([encodeVarint(5), encodeString(reason.message)]);
    }
  }
}

function encodeRelay(message: RelayMessage): Uint8Array {
  switch (message.kind) {
    case "ServerRegisterOk":
      return concatBytes([
        Uint8Array.of(RELAY_DISC_CTRL),
        encodeVarint(CTRL_SERVER_REGISTER_OK),
      ]);
    case "ServerRegisterErr": {
      return concatBytes([
        Uint8Array.of(RELAY_DISC_CTRL),
        encodeVarint(CTRL_SERVER_REGISTER_ERR),
        encodeString(message.reason),
      ]);
    }
    case "PairInbound":
      return concatBytes([
        Uint8Array.of(RELAY_DISC_CTRL),
        encodeVarint(CTRL_PAIR_INBOUND),
        encodeUuid(message.pairId),
      ]);
    case "PairOpen":
      return concatBytes([
        Uint8Array.of(RELAY_DISC_CTRL),
        encodeVarint(CTRL_PAIR_OPEN),
        encodeUuid(message.pairId),
      ]);
    case "PairClose":
      return concatBytes([
        Uint8Array.of(RELAY_DISC_CTRL),
        encodeVarint(CTRL_PAIR_CLOSE),
        encodeUuid(message.pairId),
        encodePairCloseReason(message.reason),
      ]);
    case "DataForward":
      return concatBytes([
        Uint8Array.of(RELAY_DISC_DATA),
        encodeVarint(0),
        encodeUuid(message.pairId),
        encodeByteArray(message.bytes),
      ]);
    default:
      throw new Error(`encoding ${message.kind} is not supported`);
  }
}

function randomPairId(): string {
  return crypto.randomUUID().toLowerCase();
}

function getAttachment(ws: WebSocket): SocketAttachment | null {
  try {
    const value = (ws as RelayWebSocket).deserializeAttachment();
    return value && typeof value === "object" ? (value as SocketAttachment) : null;
  } catch {
    return null;
  }
}

function setAttachment(ws: WebSocket, attachment: SocketAttachment): void {
  (ws as RelayWebSocket).serializeAttachment(attachment);
}

function bearerToken(request: Request): string | null {
  const value = request.headers.get("authorization");
  if (!value) return null;
  const [scheme, token] = value.split(/\s+/, 2);
  if (scheme?.toLowerCase() !== "bearer" || !token) {
    return null;
  }
  return token.trim();
}

function serverAuthAllowed(request: Request, env: Env): boolean {
  const configured = env.SERVER_AUTH_TOKEN?.trim();
  if (!configured) return true;
  return bearerToken(request) === configured;
}

export class RelaySessionDurableObject {
  private readonly limits: RelayLimits;
  private pairs = new Map<string, PairState>();
  private sweepScheduled = false;

  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {
    this.limits = readLimits(env);
  }

  private serverSockets(): WebSocket[] {
    return this.state.getWebSockets("server");
  }

  private clientSockets(): WebSocket[] {
    return this.state.getWebSockets("client");
  }

  private registeredServerCount(): number {
    return this.serverSockets().filter((ws) => {
      const attachment = getAttachment(ws);
      return attachment?.socketRole === "server" && attachment.registered;
    }).length;
  }

  private findRegisteredServer(serverId: string): WebSocket | null {
    for (const ws of this.serverSockets()) {
      const attachment = getAttachment(ws);
      if (
        attachment?.socketRole === "server" &&
        attachment.registered &&
        attachment.serverId === serverId
      ) {
        return ws;
      }
    }
    return null;
  }

  private clientsForPair(pairId: string): WebSocket[] {
    return this.clientSockets().filter((ws) => {
      const attachment = getAttachment(ws);
      return attachment?.socketRole === "client" && attachment.pairId === pairId;
    });
  }

  private clientsForServer(serverId: string): WebSocket[] {
    return this.clientSockets().filter((ws) => {
      const attachment = getAttachment(ws);
      return attachment?.socketRole === "client" && attachment.targetServerId === serverId;
    });
  }

  private send(ws: WebSocket, message: RelayMessage): boolean {
    try {
      ws.send(encodeRelay(message));
      return true;
    } catch {
      return false;
    }
  }

  private touchAttachment(ws: WebSocket, attachment: SocketAttachment): void {
    attachment.lastProgressAt = Date.now();
    setAttachment(ws, attachment);
  }

  private bumpRate(pair: PairState, bytes: number): string | null {
    const now = Date.now();
    if (now - pair.windowStartedAt >= 1_000) {
      pair.windowStartedAt = now;
      pair.bytesThisWindow = 0;
    }
    if (pair.bytesThisWindow + bytes > this.limits.maxBytesPerSec) {
      return "max_bytes_per_sec exceeded";
    }
    pair.bytesThisWindow += bytes;
    pair.lastProgressAt = now;
    return null;
  }

  private closePair(pairId: string, reason: CloseReason): void {
    const pair = this.pairs.get(pairId);
    if (pair) {
      this.pairs.delete(pairId);
      void this.state.storage.delete(`pair:${pairId}`);
    }

    for (const clientWs of this.clientsForPair(pairId)) {
      this.send(clientWs, { kind: "PairClose", pairId, reason });
      try {
        clientWs.close(1000, reason.type);
      } catch {
        // ignore
      }
    }

    const serverId = pair?.serverId;
    if (serverId) {
      const serverWs = this.findRegisteredServer(serverId);
      if (serverWs) {
        this.send(serverWs, { kind: "PairClose", pairId, reason });
      }
    }
  }

  private scheduleSweep(): void {
    if (this.sweepScheduled) {
      return;
    }
    this.sweepScheduled = true;
    const intervalMs = Math.max(1_000, Math.floor((this.limits.idleSeconds * 1_000) / 4));
    setTimeout(() => {
      this.sweepScheduled = false;
      this.sweepIdlePairs();
      this.scheduleSweep();
    }, intervalMs);
  }

  private sweepIdlePairs(): void {
    const now = Date.now();
    const thresholdMs = this.limits.idleSeconds * 1_000;
    for (const [pairId, pair] of this.pairs) {
      if (now - pair.lastProgressAt > thresholdMs) {
        this.closePair(pairId, { type: "Stuck" });
      }
    }
  }

  async fetch(request: Request): Promise<Response> {
    this.scheduleSweep();

    const url = new URL(request.url);
    const socketRole = parseSocketRole(url.pathname);
    if (!socketRole) {
      return new Response("Not found", { status: 404 });
    }

    const upgradeError = requireWebSocketUpgrade(request);
    if (upgradeError) {
      return upgradeError;
    }

    if (socketRole === "server" && !serverAuthAllowed(request, this.env)) {
      return new Response("Unauthorized server relay request", { status: 401 });
    }

    const attachment = initialAttachment(request);
    if (!attachment) {
      return new Response("Invalid websocket relay request", { status: 400 });
    }

    const [clientSocket, serverSocket] = getWebSocketPair();
    this.state.acceptWebSocket(serverSocket, [socketRole]);
    setAttachment(serverSocket, attachment);
    return responseWithWebSocket(clientSocket);
  }

  webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): void {
    const attachment = getAttachment(ws);
    if (!attachment) {
      ws.close(1011, "Missing relay attachment");
      return;
    }

    if (typeof message === "string") {
      if (attachment.socketRole === "server" && attachment.registered && message === "ping") {
        this.touchAttachment(ws, attachment);
        ws.send("pong");
        return;
      }
      ws.close(1003, "Text frames are not supported");
      return;
    }

    const bytes = asBytes(message);
    if (!bytes) {
      ws.close(1003, "Unsupported websocket payload");
      return;
    }

    if (attachment.socketRole === "server") {
      const decoded = decodeRelay(bytes);
      if (!attachment.registered) {
        if (decoded.kind !== "ServerRegister") {
          ws.close(1008, "Expected ServerRegister as first server frame");
          return;
        }
        if (attachment.serverId != null && decoded.serverId !== attachment.serverId) {
          this.send(ws, {
            kind: "ServerRegisterErr",
            reason: "server id URL/query mismatch",
          });
          ws.close(1008, "server id URL/query mismatch");
          return;
        }
        if (this.registeredServerCount() >= this.limits.maxServers) {
          this.send(ws, {
            kind: "ServerRegisterErr",
            reason: "max_servers exceeded",
          });
          ws.close(1008, "max_servers exceeded");
          return;
        }
        if (this.findRegisteredServer(decoded.serverId)) {
          this.send(ws, {
            kind: "ServerRegisterErr",
            reason: "duplicate server registration",
          });
          ws.close(1008, "duplicate server registration");
          return;
        }

        setAttachment(ws, {
          socketRole: "server",
          registered: true,
          serverId: decoded.serverId,
          connectedAt: attachment.connectedAt,
          lastProgressAt: Date.now(),
        });
        this.send(ws, { kind: "ServerRegisterOk" });
        return;
      }

      this.touchAttachment(ws, attachment);

      if (decoded.kind === "ServerHeartbeat") {
        return;
      }

      if (decoded.kind === "PairClose") {
        this.closePair(decoded.pairId, decoded.reason);
        return;
      }

      if (decoded.kind !== "DataForward") {
        ws.close(1008, "Unexpected relay frame on registered server socket");
        return;
      }

      const pair = this.pairs.get(decoded.pairId);
      if (!pair) {
        this.send(ws, {
          kind: "PairClose",
          pairId: decoded.pairId,
          reason: { type: "ClientGone" },
        });
        return;
      }
      const rateError = this.bumpRate(pair, decoded.bytes.length);
      if (rateError) {
        this.closePair(decoded.pairId, { type: "Rejected", message: rateError });
        return;
      }

      for (const clientWs of this.clientsForPair(decoded.pairId)) {
        const clientAttachment = getAttachment(clientWs);
        if (clientAttachment?.socketRole === "client") {
          this.touchAttachment(clientWs, clientAttachment);
        }
        try {
          clientWs.send(decoded.bytes);
        } catch {
          // ignore
        }
      }
      return;
    }

    if (attachment.pairId == null) {
      const decoded = decodeRelay(bytes);
      if (decoded.kind !== "ClientConnect") {
        ws.close(1008, "Expected ClientConnect as first client frame");
        return;
      }

      if (decoded.serverId !== attachment.targetServerId) {
        ws.close(1008, "ClientConnect server mismatch");
        return;
      }

      const serverWs = this.findRegisteredServer(attachment.targetServerId);
      const pairId = randomPairId();
      if (!serverWs) {
        this.send(ws, {
          kind: "PairClose",
          pairId,
          reason: { type: "Rejected", message: "target server not registered" },
        });
        ws.close(1008, "target server not registered");
        return;
      }
      if (this.pairs.size >= this.limits.maxPairs) {
        this.send(ws, {
          kind: "PairClose",
          pairId,
          reason: { type: "Rejected", message: "max_pairs exceeded" },
        });
        ws.close(1008, "max_pairs exceeded");
        return;
      }

      const pair: PairState = {
        pairId,
        serverId: attachment.targetServerId,
        createdAt: Date.now(),
        lastProgressAt: Date.now(),
        queuedBytes: 0,
        bytesThisWindow: 0,
        windowStartedAt: Date.now(),
      };
      this.pairs.set(pairId, pair);
      void this.state.storage.put(`pair:${pairId}`, pair);

      setAttachment(ws, {
        socketRole: "client",
        targetServerId: attachment.targetServerId,
        pairId,
        connectedAt: attachment.connectedAt,
        lastProgressAt: Date.now(),
      });

      this.send(serverWs, { kind: "PairInbound", pairId });
      this.send(ws, { kind: "PairOpen", pairId });
      return;
    }

    const pair = this.pairs.get(attachment.pairId);
    if (!pair) {
      ws.close(1008, "Unknown pair id");
      return;
    }

    try {
      const decoded = decodeRelay(bytes);
      if (decoded.kind === "PairClose") {
        this.closePair(decoded.pairId, decoded.reason);
        return;
      }
    } catch {
      // Treat non-relay bytes as opaque client payload.
    }

    const serverWs = this.findRegisteredServer(attachment.targetServerId);
    if (!serverWs) {
      this.closePair(attachment.pairId, { type: "ServerGone" });
      return;
    }

    const rateError = this.bumpRate(pair, bytes.length);
    if (rateError) {
      this.closePair(attachment.pairId, { type: "Rejected", message: rateError });
      return;
    }
    pair.queuedBytes += bytes.length;
    if (pair.queuedBytes > this.limits.maxQueuedBytes) {
      this.closePair(attachment.pairId, {
        type: "Rejected",
        message: "max_queued_bytes exceeded",
      });
      return;
    }

    this.touchAttachment(ws, attachment);
    const serverAttachment = getAttachment(serverWs);
    if (serverAttachment?.socketRole === "server") {
      this.touchAttachment(serverWs, serverAttachment);
    }

    this.send(serverWs, {
      kind: "DataForward",
      pairId: attachment.pairId,
      bytes,
    });
    pair.queuedBytes = Math.max(0, pair.queuedBytes - bytes.length);
    void this.state.storage.put(`pair:${attachment.pairId}`, pair);
  }

  webSocketClose(ws: WebSocket, _code: number, _reason: string, _wasClean: boolean): void {
    const attachment = getAttachment(ws);
    if (!attachment) {
      return;
    }

    if (attachment.socketRole === "server") {
      if (!attachment.registered || !attachment.serverId) {
        return;
      }
      for (const clientWs of this.clientsForServer(attachment.serverId)) {
        const clientAttachment = getAttachment(clientWs);
        if (clientAttachment?.socketRole === "client" && clientAttachment.pairId) {
          this.send(clientWs, {
            kind: "PairClose",
            pairId: clientAttachment.pairId,
            reason: { type: "ServerGone" },
          });
        }
        try {
          clientWs.close(1012, "Server disconnected");
        } catch {
          // ignore
        }
      }
      return;
    }

    if (attachment.pairId) {
      const serverWs = this.findRegisteredServer(attachment.targetServerId);
      if (serverWs) {
        this.send(serverWs, {
          kind: "PairClose",
          pairId: attachment.pairId,
          reason: { type: "ClientGone" },
        });
      }
      this.pairs.delete(attachment.pairId);
      void this.state.storage.delete(`pair:${attachment.pairId}`);
    }
  }

  webSocketError(_ws: WebSocket, error: unknown): void {
    console.error("[relay-cloudflare] websocket error", error);
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);

    if (url.pathname === "/health") {
      return json({
        status: "ok",
        runtime: "cloudflare-workers",
        mode: "durable-object-relay",
      });
    }

    if (url.pathname === "/ws/server" || url.pathname === "/ws/client") {
      const serverId =
        url.pathname === "/ws/server" ? getServerIdFromUrl(url) : getClientTargetServerId(url);
      if (!serverId) {
        return new Response("missing or invalid server query parameter", { status: 400 });
      }
      const id = env.RELAY.idFromName(`relay-server:${serverId}`);
      const stub = env.RELAY.get(id);
      return stub.fetch(request);
    }

    return new Response("Not found", { status: 404 });
  },
};
