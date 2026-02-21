/**
 * Typed domain errors for the Soroban Contract Client.
 *
 * All errors thrown by `SorobanContractClient` are instances of
 * `SorobanClientError`, carrying a `code` that consumers can switch on and a
 * `retryable` flag indicating whether the operation can be safely retried.
 */

export enum SorobanErrorCode {
  // ── Network / RPC ──────────────────────────────────────────────────────
  /** Generic network connectivity failure (timeout, DNS, etc.). */
  NetworkError = "NETWORK_ERROR",
  /** The Soroban RPC server returned an unexpected error response. */
  RpcError = "RPC_ERROR",
  /** Transaction simulation indicated it would fail on-chain. */
  SimulationFailed = "SIMULATION_FAILED",
  /** Transaction was submitted but was not included or failed on-chain. */
  TransactionFailed = "TX_FAILED",

  // ── Wallet ─────────────────────────────────────────────────────────────
  /** No wallet is connected; user must connect before calling write methods. */
  WalletNotConnected = "WALLET_NOT_CONNECTED",
  /** The wallet is connected to a different Stellar network. */
  NetworkMismatch = "NETWORK_MISMATCH",
  /** The user declined to sign the transaction in their wallet. */
  UserRejected = "USER_REJECTED",

  // ── Contract ───────────────────────────────────────────────────────────
  /**
   * The contract returned an error code (e.g., NotAuthorized, BadgeNotFound).
   * The `contractErrorCode` field carries the raw u32 value from Soroban.
   */
  ContractError = "CONTRACT_ERROR",

  // ── Validation ─────────────────────────────────────────────────────────
  /** A caller-supplied parameter failed pre-call validation. */
  InvalidParameter = "INVALID_PARAMETER",
  /** A required contract address is missing from the registry. */
  ContractAddressNotFound = "CONTRACT_ADDRESS_NOT_FOUND",

  // ── Retry ──────────────────────────────────────────────────────────────
  /** All retry attempts have been exhausted without a successful result. */
  RetryExhausted = "RETRY_EXHAUSTED",
}

/** Named Soroban contract error codes mapped to human-readable names. */
export const AchievementBadgeErrors: Record<number, string> = {
  1: "AlreadyInitialized",
  2: "NotInitialized",
  3: "NotAuthorized",
  4: "BadgeNotFound",
  5: "BadgeAlreadyExists",
  6: "BadgeAlreadyAwarded",
  7: "InvalidInput",
};

export const PrizePoolErrors: Record<number, string> = {
  1: "AlreadyInitialized",
  2: "NotInitialized",
  3: "NotAuthorized",
  4: "InvalidAmount",
  5: "InsufficientFunds",
  6: "GameAlreadyReserved",
  7: "ReservationNotFound",
  8: "PayoutExceedsReservation",
  9: "Overflow",
};

/**
 * Domain error thrown by `SorobanContractClient` for every failure path.
 *
 * @example
 * ```ts
 * const result = await client.badge_award(admin, user, 1n);
 * if (!result.success) {
 *   if (result.error.code === SorobanErrorCode.ContractError) {
 *     console.error("Contract error code:", result.error.contractErrorCode);
 *   }
 * }
 * ```
 */
export class SorobanClientError extends Error {
  /** Machine-readable error category. */
  readonly code: SorobanErrorCode;

  /**
   * Whether this error class is safe to retry automatically.
   * Terminal errors (contract logic failures, invalid params) are `false`.
   */
  readonly retryable: boolean;

  /**
   * Raw u32 error code returned by the contract when `code` is
   * `SorobanErrorCode.ContractError`.  `undefined` for all other codes.
   */
  readonly contractErrorCode?: number;

  /** The original SDK or network exception that caused this error. */
  readonly originalError?: unknown;

  constructor(opts: {
    code: SorobanErrorCode;
    message: string;
    retryable?: boolean;
    contractErrorCode?: number;
    originalError?: unknown;
  }) {
    super(opts.message);
    this.name = "SorobanClientError";
    this.code = opts.code;
    this.retryable = opts.retryable ?? false;
    this.contractErrorCode = opts.contractErrorCode;
    this.originalError = opts.originalError;

    // Restore prototype chain for `instanceof` checks in transpiled code.
    Object.setPrototypeOf(this, new.target.prototype);
  }

  /** Convenience factory: wallet not connected. */
  static walletNotConnected(): SorobanClientError {
    return new SorobanClientError({
      code: SorobanErrorCode.WalletNotConnected,
      message: "No wallet is connected. Connect a wallet before signing transactions.",
      retryable: false,
    });
  }

  /** Convenience factory: network mismatch. */
  static networkMismatch(expected: string, actual: string): SorobanClientError {
    return new SorobanClientError({
      code: SorobanErrorCode.NetworkMismatch,
      message: `Wallet is on network "${actual}" but client expects "${expected}".`,
      retryable: false,
    });
  }

  /** Convenience factory: invalid parameter. */
  static invalidParam(paramName: string, reason: string): SorobanClientError {
    return new SorobanClientError({
      code: SorobanErrorCode.InvalidParameter,
      message: `Invalid parameter "${paramName}": ${reason}`,
      retryable: false,
    });
  }

  /** Convenience factory: contract address not found in registry. */
  static addressNotFound(contractName: string): SorobanClientError {
    return new SorobanClientError({
      code: SorobanErrorCode.ContractAddressNotFound,
      message: `Contract address for "${contractName}" is not set in the registry.`,
      retryable: false,
    });
  }
}
