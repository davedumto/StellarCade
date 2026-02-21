# Higher or Lower Contract

A simple prediction game for Stellarcade. Players wager on whether the outcome
will be higher or lower than a fixed anchor value.

## Rules

- **Anchor value**: `50`
- **Outcome**: Provided by the RNG contract.
- **Win condition**:
  - `Higher` wins if `outcome > 50`
  - `Lower` wins if `outcome < 50`
  - `outcome == 50` is treated as a loss
- **Payout**: `2x` wager on win, `0` on loss

## Public Interface

- `init(admin, rng_contract, prize_pool_contract, balance_contract)`
- `place_prediction(player, prediction, wager, game_id)`
- `resolve_game(game_id)`
- `get_game(game_id)`

## Settlement

- On `place_prediction`, the wager is debited from the player and credited to
  the contract’s internal house balance in the User Balance contract.
- On `resolve_game`, winners are paid from the house balance.

## Validation & Safety

- Prediction values must be `0` (Higher) or `1` (Lower).
- Wager must be between `MIN_WAGER` and `MAX_WAGER`.
- Duplicate `game_id` values are rejected.
- Games can only be resolved once.
- Resolution requires RNG readiness (`is_ready`).

## Events

- `PredictionPlaced(game_id, player, prediction, wager)`
- `GameResolved(game_id, outcome, win, payout)`

## Tests

```bash
cd contracts/higher-lower
cargo test
```
