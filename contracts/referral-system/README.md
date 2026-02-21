# Referral System Contract

Manages a referral program for the StellarCade platform, tracking referrer-referee relationships, computing referral rewards from qualifying events, and handling reward claims.

## Overview

The referral system incentivizes user acquisition by rewarding existing users (referrers) when their referred users (referees) perform qualifying actions on the platform.

The lifecycle of a referral is:
1. **Registration**: A user registers with a referrer via `register_referrer`.
2. **Event Recording**: When the referee performs qualifying actions (game played, deposit, prize claimed), an admin records the event via `record_referral_event`.
3. **Reward Accumulation**: The referrer's pending reward balance increases based on the event amount and the configured reward percentage.
4. **Claiming**: The referrer claims accumulated rewards via `claim_referral_reward`.

Rewards are computed as a configurable percentage (in basis points) of the event amount. The default is 5% (500 bps).

## Methods

### `init(admin: Address, reward_contract: Address) → Result<(), Error>`

Initialize the referral system. May only be called once.

- `admin` — authorized to record events and update configuration.
- `reward_contract` — address of the contract/account that funds rewards.

Sets default reward percentage to 500 bps (5%).

**Event:** `Initialized { admin, reward_contract, reward_bps }`

### `register_referrer(user: Address, referrer: Address) → Result<(), Error>`

Register `referrer` as the referrer of `user`. User must authorize.

- A user cannot refer themselves.
- A user can only be referred once.
- Both user and referrer states are initialized/updated.

**Event:** `ReferrerRegistered { user, referrer }`

### `record_referral_event(admin: Address, user: Address, event_type: EventType, amount: i128) → Result<(), Error>`

Record a qualifying referral event for `user`. Admin only.

- `event_type` — one of `GamePlayed`, `Deposit`, `PrizeClaimed`.
- `amount` — the transaction value (must be > 0).
- Reward is computed as `amount * reward_bps / 10_000` and credited to the user's referrer.

**Event:** `ReferralEventRecorded { user, referrer, event_type, amount, reward }`

### `claim_referral_reward(user: Address) → Result<i128, Error>`

Claim all pending referral rewards. User must authorize.

- Returns the claimed amount.
- Pending balance is set to zero before any external interaction (reentrancy guard).

**Event:** `RewardClaimed { user, amount }`

### `referral_state(user: Address) → Result<ReferralState, Error>`

Return the full referral state for a user, including referrer, referees list, total earned, pending reward, and event count.

### `get_referrer(user: Address) → Option<Address>`

Return the referrer of a user, or `None` if not referred.

### `set_reward_bps(admin: Address, bps: u32) → Result<(), Error>`

Update the reward percentage (basis points, max 10_000). Admin only.

### `set_reward_contract(admin: Address, reward_contract: Address) → Result<(), Error>`

Update the reward contract address. Admin only.

### `get_reward_contract() → Result<Address, Error>`

Return the configured reward contract address.

### `get_reward_bps() → Result<u32, Error>`

Return the current reward percentage in basis points.

---

## Events

| Event | Topics | Data | Description |
|-------|--------|------|-------------|
| `Initialized` | `admin` | `reward_contract`, `reward_bps` | Contract initialized |
| `ReferrerRegistered` | `user`, `referrer` | — | Referral relationship created |
| `ReferralEventRecorded` | `user`, `referrer` | `event_type`, `amount`, `reward` | Qualifying event recorded |
| `RewardClaimed` | `user` | `amount` | Reward claimed by referrer |

---

## Storage

| Key | Kind | Type | Description |
|-----|------|------|-------------|
| `Admin` | instance | `Address` | Platform administrator |
| `RewardContract` | instance | `Address` | Reward funding contract |
| `RewardBps` | instance | `u32` | Reward percentage in basis points |
| `State(addr)` | persistent | `ReferralState` | Per-user referral state |
| `ReferredBy(addr)` | persistent | `Address` | Referee → referrer mapping |

TTL for persistent entries is bumped to ~30 days (`518_400` ledgers) on every write.

---

## Error Codes

| Code | Name | Description |
|------|------|-------------|
| 1 | `AlreadyInitialized` | `init` called more than once |
| 2 | `NotInitialized` | Method called before `init` |
| 3 | `NotAuthorized` | Caller is not the admin |
| 4 | `InvalidAmount` | Amount ≤ 0 or bps > 10_000 |
| 5 | `AlreadyReferred` | User already has a referrer |
| 6 | `SelfReferral` | User attempted to refer themselves |
| 7 | `ReferrerNotRegistered` | User has no referrer or state not found |
| 8 | `NoPendingRewards` | No rewards available to claim |
| 9 | `AlreadyClaimed` | Reserved for future use |
| 10 | `InvalidEventType` | Reserved for future use |
| 99 | `Overflow` | Arithmetic overflow |

---

## Invariants

- A user can only have one referrer (immutable once set).
- `total_earned` always equals the sum of all rewards ever credited.
- `pending_reward` is always ≥ 0.
- `pending_reward` is zeroed **before** any external call (reentrancy safety).
- `event_count` monotonically increases.

---

## Integration Assumptions

- **Reward Settlement**: `RewardClaimed` events trigger off-chain or cross-contract token transfers via `RewardContract`.
- **Event Recording**: An authorized admin/operator (e.g., game server) calls `record_referral_event` when qualifying actions occur.
- **Depends on**: Issues #25, #26, #27, #28, and #36 for stable platform-wide integration.
