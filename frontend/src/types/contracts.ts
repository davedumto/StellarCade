/**
 * Shared contract types and result envelope for the Soroban Contract Client.
 *
 * All public methods on `SorobanContractClient` return `ContractResult<T>`,
 * a discriminated union that forces callers to handle both success and failure
 * paths at the type level.
 */

import type { SorobanClientError } from "./errors";

// ── Result Envelope ────────────────────────────────────────────────────────────

/**
 * Unified result type for every contract call.
 *
 * @template T The success data type.
 *
 * @example
 * ```ts
 * const result = await client.pool_getState();
 * if (result.success) {
 *   console.log("Available:", result.data.available);
 * } else {
 *   console.error(result.error.message);
 * }
 * ```
 */
export type ContractResult<T> =
  | {
      success: true;
      /** Deserialized return value from the contract. */
      data: T;
      /** Transaction hash — present for state-mutating calls, absent for simulations/reads. */
      txHash?: string;
      /** Ledger sequence number at which the transaction was included. */
      ledger?: number;
    }
  | {
      success: false;
      error: SorobanClientError;
    };

// ── Call Options ───────────────────────────────────────────────────────────────

/**
 * Per-call options accepted by every `SorobanContractClient` method.
 */
export interface CallOptions {
  /**
   * Base fee in stroops (1 XLM = 10,000,000 stroops).
   * @default 100
   */
  fee?: number;

  /**
   * Transaction validity window in seconds.
   * @default 30
   */
  timeoutSecs?: number;

  /**
   * Maximum number of automatic retries for retryable errors.
   * Set to 0 to disable retries.
   * @default 3
   */
  retries?: number;

  /**
   * Caller-provided idempotency key.  The client logs a warning if the same
   * key is used for more than one invocation within a session, helping callers
   * detect accidental duplicate submissions.
   */
  idempotencyKey?: string;
}

// ── Wallet Provider Interface ──────────────────────────────────────────────────

/**
 * Minimal wallet abstraction consumed by `SorobanContractClient`.
 *
 * Both the Freighter adapter and the in-memory test mock implement this
 * interface, enabling deterministic unit testing without a browser extension.
 */
export interface WalletProvider {
  /** Returns `true` when a wallet is actively connected. */
  isConnected(): Promise<boolean>;

  /** Returns the connected wallet's G... public key. */
  getPublicKey(): Promise<string>;

  /**
   * Returns the network the wallet is currently configured for.
   * Used to guard against cross-network transaction signing.
   */
  getNetwork(): Promise<{ network: string; networkPassphrase: string }>;

  /**
   * Signs `xdr` with the wallet's private key and returns the signed XDR
   * string.  Rejects with a user-facing error if the user declines.
   */
  signTransaction(
    xdr: string,
    opts?: { network?: string; networkPassphrase?: string }
  ): Promise<string>;
}

// ── AchievementBadge Types ────────────────────────────────────────────────────

/**
 * Parameters for `badge_define`.
 *
 * `criteriaHash` must be a 64-character lowercase hex string representing the
 * 32-byte SHA-256 hash of the off-chain criteria document.
 */
export interface DefineBadgeParams {
  /** Unique badge identifier (u64, represented as bigint). */
  badgeId: bigint;
  /**
   * 64-char hex SHA-256 hash of the off-chain criteria document.
   * @example "a3f5...c1d2"
   */
  criteriaHash: string;
  /**
   * Token reward amount disbursed on award.  Use `0n` for no reward.
   * Must be ≥ 0.
   */
  reward: bigint;
}

/**
 * On-chain badge definition returned by read operations.
 */
export interface BadgeDefinition {
  /** Raw 32-byte criteria hash as a hex string. */
  criteriaHash: string;
  /** Token reward amount (0 = no reward). */
  reward: bigint;
}

// ── PrizePool Types ───────────────────────────────────────────────────────────

/**
 * Point-in-time snapshot of the prize pool's accounting state.
 * Returned by `pool_getState`.
 */
export interface PoolState {
  /** Tokens available for new game reservations. */
  available: bigint;
  /** Tokens currently earmarked across all active game reservations. */
  reserved: bigint;
}

// ── AccessControl Types ───────────────────────────────────────────────────────

/**
 * Predefined role symbols matching those in the access-control contract.
 */
export enum ContractRole {
  Admin = "ADMIN",
  Operator = "OPERATOR",
  Pauser = "PAUSER",
  Game = "GAME",
}
