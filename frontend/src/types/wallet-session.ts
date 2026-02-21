/*
 * Typed contracts for Wallet Session Service
 */

export type Network = "TESTNET" | "PUBLIC" | string;

export interface WalletProviderInfo {
  id: string; // provider identifier (e.g. 'freighter', 'solflare')
  name: string;
  version?: string;
}

export interface WalletSessionMeta {
  provider: WalletProviderInfo;
  address: string;
  network: Network;
  connectedAt: number; // epoch ms
  lastActiveAt?: number; // epoch ms
}

export enum WalletSessionState {
  DISCONNECTED = "DISCONNECTED",
  CONNECTING = "CONNECTING",
  CONNECTED = "CONNECTED",
  RECONNECTING = "RECONNECTING",
}

// Domain errors
export class WalletSessionError extends Error {
  public code: string;
  constructor(code: string, message?: string) {
    super(message ?? code);
    this.code = code;
    this.name = "WalletSessionError";
  }
}

export class ProviderNotFoundError extends WalletSessionError {
  constructor() {
    super("provider_not_found", "Wallet provider not found");
  }
}

export class RejectedSignatureError extends WalletSessionError {
  constructor() {
    super("rejected_signature", "User rejected the signature request");
  }
}

export class StaleSessionError extends WalletSessionError {
  constructor() {
    super("stale_session", "Stored session is stale or invalid");
  }
}

export class ValidationError extends WalletSessionError {
  constructor(message?: string) {
    super("validation_error", message ?? "Invalid parameters");
  }
}

export interface WalletSessionOptions {
  storageKey?: string; // override localStorage key
  supportedNetworks?: Network[];
  sessionExpiryMs?: number; // how long a stored session remains valid
}
