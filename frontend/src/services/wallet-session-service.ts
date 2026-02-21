import {
  WalletProviderInfo,
  WalletSessionMeta,
  WalletSessionOptions,
  WalletSessionState,
  ProviderNotFoundError,
  RejectedSignatureError,
  StaleSessionError,
  ValidationError,
  WalletSessionError,
} from "../types/wallet-session";

type Subscriber = (
  state: WalletSessionState,
  meta?: WalletSessionMeta | null,
  error?: Error | null,
) => void;

const DEFAULT_KEY = "stc_wallet_session_v1";
const DEFAULT_EXPIRY = 1000 * 60 * 60 * 24 * 7; // 7 days

// Minimal adapter interface for injected wallet providers.
export interface WalletProviderAdapter {
  isAvailable(): boolean;
  connect(): Promise<{
    address: string;
    provider: WalletProviderInfo;
    network: string;
  }>;
  disconnect?(): Promise<void>;
  // signMessage is optional; used to verify ownership when restoring
  signMessage?(message: string): Promise<string>;
}

export class WalletSessionService {
  private storageKey: string;
  private sessionExpiryMs: number;
  private supportedNetworks: string[] | undefined;
  private providerAdapter: WalletProviderAdapter | null = null;
  private state: WalletSessionState = WalletSessionState.DISCONNECTED;
  private meta: WalletSessionMeta | null = null;
  private subscribers: Set<Subscriber> = new Set();

  constructor(opts?: WalletSessionOptions) {
    this.storageKey = opts?.storageKey ?? DEFAULT_KEY;
    this.sessionExpiryMs = opts?.sessionExpiryMs ?? DEFAULT_EXPIRY;
    this.supportedNetworks = opts?.supportedNetworks as string[] | undefined;
    // try to restore on construction (non-blocking)
    try {
      const restored = this.restore();
      if (restored) {
        this.meta = restored;
      }
    } catch (e) {
     
    }
  }

  public setProviderAdapter(adapter: WalletProviderAdapter) {
    this.providerAdapter = adapter;
  }

  public subscribe(fn: Subscriber) {
    this.subscribers.add(fn);
    // send current state immediately
    fn(this.state, this.meta, null);
    return () => {
      // ensure cleanup returns void (React expects void | Destructor)
      this.subscribers.delete(fn);
    };
  }

  private notify(error: Error | null = null) {
    for (const s of this.subscribers) {
      try {
        s(this.state, this.meta, error);
      } catch (e) {
        console.error("Subscriber error", e);
      }
    }
  }

  private persist(meta: WalletSessionMeta | null) {
    if (!meta) {
      localStorage.removeItem(this.storageKey);
      return;
    }
    const payload = {
      meta,
      storedAt: Date.now(),
    };
    localStorage.setItem(this.storageKey, JSON.stringify(payload));
  }

  private restore(): WalletSessionMeta | null {
    const raw = localStorage.getItem(this.storageKey);
    if (!raw) return null;
    try {
      const parsed = JSON.parse(raw) as {
        meta: WalletSessionMeta;
        storedAt: number;
      };
      if (typeof parsed?.storedAt !== "number" || !parsed?.meta)
        throw new Error("invalid");
      if (Date.now() - parsed.storedAt > this.sessionExpiryMs) {
        this.persist(null);
        throw new StaleSessionError();
      }
      if (
        this.supportedNetworks &&
        !this.supportedNetworks.includes(parsed.meta.network)
      ) {
        this.persist(null);
        throw new StaleSessionError();
      }
      return parsed.meta;
    } catch (e) {
      localStorage.removeItem(this.storageKey);
      return null;
    }
  }

  public async connect(options?: {
    network?: string;
  }): Promise<WalletSessionMeta> {
    if (!this.providerAdapter) {
      throw new ProviderNotFoundError();
    }
    if (!this.providerAdapter.isAvailable()) {
      throw new ProviderNotFoundError();
    }

    this.state = WalletSessionState.CONNECTING;
    this.notify();

    if (
      options?.network &&
      this.supportedNetworks &&
      !this.supportedNetworks.includes(options.network)
    ) {
      this.state = WalletSessionState.DISCONNECTED;
      const err = new ValidationError("Network not supported");
      this.notify(err);
      throw err;
    }

    try {
      const res = await this.providerAdapter.connect();
      if (!res || !res.address) throw new Error("connect_no_address");

      const meta: WalletSessionMeta = {
        provider: res.provider,
        address: res.address,
        network: options?.network ?? (res.network as string) ?? "UNKNOWN",
        connectedAt: Date.now(),
        lastActiveAt: Date.now(),
      };

      this.meta = meta;
      this.state = WalletSessionState.CONNECTED;
      this.persist(meta);
      this.notify();
      return meta;
    } catch (e: any) {
      this.state = WalletSessionState.DISCONNECTED;
      const err = this.mapError(e);
      this.notify(err);
      throw err;
    }
  }

  // Disconnect and clear persisted session
  public async disconnect(): Promise<void> {
    this.state = WalletSessionState.DISCONNECTED;
    try {
      if (this.providerAdapter?.disconnect) {
        await this.providerAdapter.disconnect();
      }
    } catch (e) {
      // ignore provider disconnect errors but notify
      const err = this.mapError(e as Error);
      this.persist(null);
      this.meta = null;
      this.notify(err);
      return;
    }
    this.persist(null);
    this.meta = null;
    this.notify();
  }


  public async reconnect(): Promise<WalletSessionMeta> {
    if (!this.providerAdapter) throw new ProviderNotFoundError();
    const stored = this.restore();
    if (!stored) throw new StaleSessionError();

    this.state = WalletSessionState.RECONNECTING;
    this.notify();

    try {
      if (this.providerAdapter.signMessage) {
        const challenge = `stc_reconnect:${stored.address}:${Math.random().toString(36).slice(2)}`;
        try {
          await this.providerAdapter.signMessage(challenge);
        } catch (e) {
          const mapped = this.mapError(e as Error);
          this.state = WalletSessionState.DISCONNECTED;
          this.notify(mapped);
          throw mapped;
        }
      }

      // If reached here, treat as connected
      const meta = { ...stored, lastActiveAt: Date.now() };
      this.meta = meta;
      this.state = WalletSessionState.CONNECTED;
      this.persist(meta);
      this.notify();
      return meta;
    } catch (e: any) {
      this.state = WalletSessionState.DISCONNECTED;
      const err = this.mapError(e);
      this.notify(err);
      throw err;
    }
  }

  public getState() {
    return this.state;
  }

  public getMeta(): WalletSessionMeta | null {
    return this.meta;
  }

  private mapError(e: Error): Error {
    if (!e) return new WalletSessionError("unknown_error", "Unknown error");
    const msg = (e as any).message ?? "";
    if (msg.includes("User rejected") || msg.includes("rejected"))
      return new RejectedSignatureError();
    if (msg.includes("provider") || msg.includes("not found"))
      return new ProviderNotFoundError();
    if (e instanceof WalletSessionError) return e;
    return new WalletSessionError("unknown_error", msg);
  }
}

export default WalletSessionService;
