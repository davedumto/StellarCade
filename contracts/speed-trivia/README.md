# Stellarcade Speed Trivia Contract

The Speed Trivia contract allows administrators to host trivia rounds where players compete to provide correct answers within a specified deadline. Rewards are shared among all winners in a round.

## 🚀 Features

- **Prize Pool Integration**: Automatically reserves and payouts prizes using the Stellarcade Prize Pool.
- **Deadline Enforcement**: Submissions are strictly rejected after the round deadline.
- **Speed Tracking**: Submissions include a timestamp to facilitate speed-based rankings (on-chain or off-chain).
- **Secure Settlement**: Prize distribution is finalized by admins and claimed by players.

## 🛠 Public Methods

### `init(admin, prize_pool_contract, balance_contract)`
Initializes the contract with the administrator address and dependent contract addresses.

### `open_question(round_id, answer_commitment, deadline, reward_amount)`
Opens a new trivia round. Reserves the `reward_amount` in the prize pool.
- `round_id`: Unique identifier for the round.
- `answer_commitment`: SHA-256 hash of the correct answer.
- `deadline`: Ledger timestamp after which no more answers are accepted.
- `reward_amount`: Total prize pool for the round.

### `submit_answer(player, round_id, answer, timestamp)`
Submits an answer for an open round.
- `player`: Address of the player (requires authorization).
- `answer`: The plaintext answer (hashed on-chain to verify against commitment).
- `timestamp`: The submission time provided by the caller (validated against ledger).

### `finalize_round(round_id)`
Closes the round for submissions and calculates the payout per winner. If no winners exist, funds are released back to the prize pool.

### `claim_reward(player, round_id)`
Allows a winner to claim their share of the prize pool after the round is finalized.

## 📊 Storage

- **Instance**: Admin address, Prize Pool address, Balance contract address.
- **Persistent**: Round data (indexed by `round_id`), Submissions (indexed by `round_id` and `player`).

## 🔔 Events

- `QuestionOpened`: Emitted when a new round is created.
- `AnswerSubmitted`: Emitted when a player submits an answer.
- `RoundFinalized`: Emitted when a round is closed and payouts are calculated.
- `RewardClaimed`: Emitted when a player successfully claims their reward.

## 🛡 Invariants & Security

- Only the admin can open or finalize rounds.
- Players can only submit one answer per round.
- Answers cannot be submitted after the deadline.
- Reward claiming is only possible for correct answers in finalized rounds.
- Arithmetic is protected against overflows using `checked` operations.
