# Price Prediction Game Contract

A pari-mutuel prediction market where players wager on whether an asset's
price will go UP or DOWN within a time window. Integrated with an oracle
contract for trustworthy price feeds.

## Public Interface

| Function | Auth | Description |
|----------|------|-------------|
| `init(admin, oracle_contract, token, min_wager, max_wager, house_edge_bps)` | Admin | One-time setup |
| `open_market(round_id, asset, close_time)` | Admin | Open a new prediction round |
| `place_prediction(player, round_id, direction, wager)` | Player | Bet UP (0) or DOWN (1) |
| `settle_round(round_id)` | None | Settle after close_time passes |
| `claim(player, round_id)` | Player | Claim winnings or push refund |
| `get_round(round_id)` | None | View round state |
| `get_bet(round_id, player)` | None | View a player's bet |

## End-to-End Game Flow

```
1. Admin deploys PricePrediction + Oracle contracts
2. Admin calls init(...)

--- Per round ---

3. Admin calls open_market(round_id, "BTC", close_time)
   → Oracle queried for current price → stored as open_price

4. Players call place_prediction(player, round_id, UP/DOWN, wager)
   → Tokens transfer from player to contract (escrow)
   → Must be before close_time
   → One bet per player per round

5. After close_time, anyone calls settle_round(round_id)
   → Oracle queried for current price → stored as close_price
   → Outcome determined: UP if close > open, DOWN if close < open, FLAT if equal
   → Net pool calculated (total pool minus house fee)

6. Winners call claim(player, round_id)
   → Proportional share of net pool transferred to player
   → Push rounds: all players get full wager refund
```

## Pari-Mutuel Settlement

Unlike fixed-odds games, winners share the combined pool:

- **Total pool** = sum of all UP wagers + sum of all DOWN wagers
- **House fee** = total_pool × house_edge_bps / 10000
- **Net pool** = total_pool − fee
- **Winner's payout** = net_pool × (their_wager / total_winning_side)

Example: 500 bps (5%) edge, 300 UP total, 700 DOWN total, price goes UP:
- Total pool = 1000, fee = 50, net pool = 950
- An UP bettor who wagered 300 gets: 950 × 300/300 = **950 tokens**
- A DOWN bettor gets: **0 tokens**

## Push Rules

A round is a **push** (all bets refunded in full) when:
- Close price equals open price (flat market)
- No bets were placed
- Only one side has bets (no opposing risk)

This protects players from losing the house fee when there's no actual
market to participate in.

## Events

| Event | Topics | Fields |
|-------|--------|--------|
| `MarketOpened` | `round_id` | `asset`, `open_price`, `close_time` |
| `PredictionPlaced` | `round_id`, `player` | `direction`, `wager` |
| `RoundSettled` | `round_id` | `close_price`, `outcome`, `is_push`, `net_pool` |
| `Claimed` | `round_id`, `player` | `payout` |

## Storage

| Key | Scope | Description |
|-----|-------|-------------|
| `Admin` | Instance | Contract administrator |
| `Token` | Instance | Payment token address |
| `OracleContract` | Instance | Price oracle contract address |
| `MinWager` | Instance | Minimum allowed wager |
| `MaxWager` | Instance | Maximum allowed wager |
| `HouseEdgeBps` | Instance | House edge in basis points |
| `Round(u64)` | Persistent | Round data by round ID |
| `Bet(BetKey)` | Persistent | Per-player bet by (round_id, player) |

## Invariants

- Each `round_id` can only be used once (no duplicate rounds)
- A round can only be settled once (`settled` flag checked)
- Predictions must be placed before `close_time`
- Settlement can only happen after `close_time`
- Each player can only bet once per round
- Each player can only claim once per round
- Wagers must be within configured min/max bounds and > 0
- Directions must be 0 (UP) or 1 (DOWN)
- State is updated before external token transfers (reentrancy-safe)
- Checked arithmetic prevents overflow on all pool calculations
- Persistent storage TTL is extended on every write (~30 days)

## Security

- Admin auth enforced for `open_market`
- Player auth enforced for `place_prediction` and `claim`
- Oracle price must be > 0 when opening a market
- Close time must be in the future when opening a market
- Duplicate round IDs rejected
- Duplicate bets per player per round rejected
- Double settlement rejected
- Double claim rejected
- Predictions after close rejected
- Settlement before close rejected
- State updated before external token transfers (reentrancy-safe)
- Losers cannot claim (explicit NoPayout error)

## Dependencies

| Contract | Purpose |
|----------|---------|
| Oracle Contract | Provides asset price feeds (`get_price(asset)`) |
| Stellar Token | Wager escrow and payout transfers |

The oracle contract must implement a `get_price(asset: Symbol) -> i128`
method. The price is queried at market open (for `open_price`) and at
settlement (for `close_price`).

## Running Tests

```bash
cd contracts/price-prediction
cargo test
```
