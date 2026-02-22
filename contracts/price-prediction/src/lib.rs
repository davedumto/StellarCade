//! Stellarcade Price Prediction Contract
//!
//! A pari-mutuel prediction market where players wager on whether an
//! asset's price will go UP or DOWN within a time window.
//!
//! ## Game Flow
//! 1. Admin calls `open_market` → oracle provides opening price, round stored.
//! 2. Players call `place_prediction` before `close_time` → tokens escrowed.
//! 3. After `close_time`, anyone calls `settle_round` → oracle provides
//!    closing price, outcome determined, net pool calculated.
//! 4. Winners call `claim` → proportional share of net pool transferred.
//!
//! ## Pari-Mutuel Settlement
//! - Total pool = sum of all wagers from both sides.
//! - House fee = total_pool × house_edge_bps / 10000.
//! - Net pool = total_pool − fee.
//! - Each winner receives: net_pool × (their_wager / total_winning_side).
//!
//! ## Push Rules
//! A round is a push (all bets refunded) when:
//! - Close price equals open price (flat).
//! - No bets were placed.
//! - Only one side has bets (no opposing risk).
#![no_std]
#![allow(unexpected_cfgs)]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    token::TokenClient, Address, Env, Symbol,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const PERSISTENT_BUMP_LEDGERS: u32 = 518_400;
const BASIS_POINTS_DIVISOR: i128 = 10_000;

pub const DIRECTION_UP: u32 = 0;
pub const DIRECTION_DOWN: u32 = 1;

pub const OUTCOME_UP: u32 = 0;
pub const OUTCOME_DOWN: u32 = 1;
pub const OUTCOME_FLAT: u32 = 2;

// ---------------------------------------------------------------------------
// External contract clients
// ---------------------------------------------------------------------------

#[contractclient(name = "OracleClient")]
pub trait OracleContract {
    fn get_price(env: Env, asset: Symbol) -> i128;
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized  = 1,
    NotInitialized      = 2,
    NotAuthorized       = 3,
    InvalidAmount       = 4,
    InvalidDirection    = 5,
    RoundAlreadyExists  = 6,
    RoundNotFound       = 7,
    AlreadySettled      = 8,
    NotSettled          = 9,
    RoundNotClosed      = 10,
    RoundClosed         = 11,
    BetAlreadyPlaced    = 12,
    BetNotFound         = 13,
    AlreadyClaimed      = 14,
    NoPayout            = 15,
    WagerTooLow         = 16,
    WagerTooHigh        = 17,
    Overflow            = 18,
    InvalidCloseTime    = 19,
    InvalidPrice        = 20,
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub struct BetKey {
    pub round_id: u64,
    pub player: Address,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Token,
    OracleContract,
    MinWager,
    MaxWager,
    HouseEdgeBps,
    Round(u64),
    Bet(BetKey),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoundData {
    pub asset: Symbol,
    pub open_price: i128,
    pub close_price: i128,
    pub close_time: u64,
    pub total_up: i128,
    pub total_down: i128,
    pub settled: bool,
    pub outcome: u32,
    pub is_push: bool,
    pub net_pool: i128,
    pub winning_total: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BetData {
    pub direction: u32,
    pub wager: i128,
    pub claimed: bool,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct MarketOpened {
    #[topic]
    pub round_id: u64,
    pub asset: Symbol,
    pub open_price: i128,
    pub close_time: u64,
}

#[contractevent]
pub struct PredictionPlaced {
    #[topic]
    pub round_id: u64,
    #[topic]
    pub player: Address,
    pub direction: u32,
    pub wager: i128,
}

#[contractevent]
pub struct RoundSettled {
    #[topic]
    pub round_id: u64,
    pub close_price: i128,
    pub outcome: u32,
    pub is_push: bool,
    pub net_pool: i128,
}

#[contractevent]
pub struct Claimed {
    #[topic]
    pub round_id: u64,
    #[topic]
    pub player: Address,
    pub payout: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct PricePrediction;

#[contractimpl]
impl PricePrediction {
    /// Initialize the price prediction game.
    ///
    /// `house_edge_bps`: house edge in basis points (e.g., 500 = 5%).
    pub fn init(
        env: Env,
        admin: Address,
        oracle_contract: Address,
        token: Address,
        min_wager: i128,
        max_wager: i128,
        house_edge_bps: i128,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::OracleContract, &oracle_contract);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::MinWager, &min_wager);
        env.storage().instance().set(&DataKey::MaxWager, &max_wager);
        env.storage().instance().set(&DataKey::HouseEdgeBps, &house_edge_bps);
        Ok(())
    }

    /// Open a new prediction market round. Admin only.
    ///
    /// Queries the oracle for the current price of `asset` to set the
    /// opening price. `close_time` must be in the future.
    pub fn open_market(
        env: Env,
        round_id: u64,
        asset: Symbol,
        close_time: u64,
    ) -> Result<(), Error> {
        require_initialized(&env)?;
        require_admin(&env)?;

        if close_time <= env.ledger().timestamp() {
            return Err(Error::InvalidCloseTime);
        }

        let round_key = DataKey::Round(round_id);
        if env.storage().persistent().has(&round_key) {
            return Err(Error::RoundAlreadyExists);
        }

        // Get opening price from oracle
        let oracle_addr = get_oracle(&env);
        let open_price = OracleClient::new(&env, &oracle_addr).get_price(&asset);
        if open_price <= 0 {
            return Err(Error::InvalidPrice);
        }

        let round = RoundData {
            asset: asset.clone(),
            open_price,
            close_price: 0,
            close_time,
            total_up: 0,
            total_down: 0,
            settled: false,
            outcome: 0,
            is_push: false,
            net_pool: 0,
            winning_total: 0,
        };
        env.storage().persistent().set(&round_key, &round);
        env.storage()
            .persistent()
            .extend_ttl(&round_key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);

        MarketOpened { round_id, asset, open_price, close_time }.publish(&env);
        Ok(())
    }

    /// Player places a prediction on an open round.
    ///
    /// `direction`: 0 = Up, 1 = Down.
    /// Tokens are transferred from the player to the contract as escrow.
    /// Each player may only bet once per round.
    pub fn place_prediction(
        env: Env,
        player: Address,
        round_id: u64,
        direction: u32,
        wager: i128,
    ) -> Result<(), Error> {
        require_initialized(&env)?;
        player.require_auth();

        if direction != DIRECTION_UP && direction != DIRECTION_DOWN {
            return Err(Error::InvalidDirection);
        }
        if wager <= 0 {
            return Err(Error::InvalidAmount);
        }

        let min_wager: i128 = env.storage().instance().get(&DataKey::MinWager).unwrap();
        let max_wager: i128 = env.storage().instance().get(&DataKey::MaxWager).unwrap();
        if wager < min_wager {
            return Err(Error::WagerTooLow);
        }
        if wager > max_wager {
            return Err(Error::WagerTooHigh);
        }

        let round_key = DataKey::Round(round_id);
        let mut round: RoundData = env
            .storage()
            .persistent()
            .get(&round_key)
            .ok_or(Error::RoundNotFound)?;

        if round.settled {
            return Err(Error::AlreadySettled);
        }
        if env.ledger().timestamp() >= round.close_time {
            return Err(Error::RoundClosed);
        }

        let bet_key = DataKey::Bet(BetKey {
            round_id,
            player: player.clone(),
        });
        if env.storage().persistent().has(&bet_key) {
            return Err(Error::BetAlreadyPlaced);
        }

        // Transfer tokens from player to contract
        let token = get_token(&env);
        TokenClient::new(&env, &token).transfer(
            &player,
            env.current_contract_address(),
            &wager,
        );

        // Update round totals
        if direction == DIRECTION_UP {
            round.total_up = round.total_up.checked_add(wager).ok_or(Error::Overflow)?;
        } else {
            round.total_down = round.total_down.checked_add(wager).ok_or(Error::Overflow)?;
        }
        env.storage().persistent().set(&round_key, &round);
        env.storage()
            .persistent()
            .extend_ttl(&round_key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);

        // Store bet
        let bet = BetData {
            direction,
            wager,
            claimed: false,
        };
        env.storage().persistent().set(&bet_key, &bet);
        env.storage()
            .persistent()
            .extend_ttl(&bet_key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);

        PredictionPlaced { round_id, player, direction, wager }.publish(&env);
        Ok(())
    }

    /// Settle a round after `close_time` has passed.
    /// Anyone can call this — the outcome is deterministic from the oracle.
    ///
    /// A round is a push (all bets refunded) when:
    /// - Close price equals open price (flat market).
    /// - No bets were placed.
    /// - Only one side has bets (no opposing risk).
    pub fn settle_round(env: Env, round_id: u64) -> Result<(), Error> {
        require_initialized(&env)?;

        let round_key = DataKey::Round(round_id);
        let mut round: RoundData = env
            .storage()
            .persistent()
            .get(&round_key)
            .ok_or(Error::RoundNotFound)?;

        if round.settled {
            return Err(Error::AlreadySettled);
        }
        if env.ledger().timestamp() < round.close_time {
            return Err(Error::RoundNotClosed);
        }

        // Get closing price from oracle
        let oracle_addr = get_oracle(&env);
        let close_price = OracleClient::new(&env, &oracle_addr).get_price(&round.asset);

        let total_pool = round
            .total_up
            .checked_add(round.total_down)
            .ok_or(Error::Overflow)?;

        // Determine outcome
        let outcome = if close_price > round.open_price {
            OUTCOME_UP
        } else if close_price < round.open_price {
            OUTCOME_DOWN
        } else {
            OUTCOME_FLAT
        };

        // Push if: flat, no bets, or only one side has bets
        let is_push = outcome == OUTCOME_FLAT
            || total_pool == 0
            || round.total_up == 0
            || round.total_down == 0;

        let (net_pool, winning_total) = if is_push {
            (0i128, 0i128)
        } else {
            let house_edge_bps: i128 =
                env.storage().instance().get(&DataKey::HouseEdgeBps).unwrap();
            let fee = total_pool
                .checked_mul(house_edge_bps)
                .and_then(|v| v.checked_div(BASIS_POINTS_DIVISOR))
                .ok_or(Error::Overflow)?;
            let net = total_pool.checked_sub(fee).ok_or(Error::Overflow)?;
            let wt = if outcome == OUTCOME_UP {
                round.total_up
            } else {
                round.total_down
            };
            (net, wt)
        };

        round.close_price = close_price;
        round.settled = true;
        round.outcome = outcome;
        round.is_push = is_push;
        round.net_pool = net_pool;
        round.winning_total = winning_total;
        env.storage().persistent().set(&round_key, &round);
        env.storage()
            .persistent()
            .extend_ttl(&round_key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);

        RoundSettled { round_id, close_price, outcome, is_push, net_pool }.publish(&env);
        Ok(())
    }

    /// Claim winnings for a settled round. Winners receive their
    /// proportional share of the net pool. In a push round, all
    /// players receive a full refund of their wager.
    ///
    /// Losers cannot claim (returns `NoPayout`).
    pub fn claim(env: Env, player: Address, round_id: u64) -> Result<(), Error> {
        require_initialized(&env)?;
        player.require_auth();

        let round_key = DataKey::Round(round_id);
        let round: RoundData = env
            .storage()
            .persistent()
            .get(&round_key)
            .ok_or(Error::RoundNotFound)?;

        if !round.settled {
            return Err(Error::NotSettled);
        }

        let bet_key = DataKey::Bet(BetKey {
            round_id,
            player: player.clone(),
        });
        let mut bet: BetData = env
            .storage()
            .persistent()
            .get(&bet_key)
            .ok_or(Error::BetNotFound)?;

        if bet.claimed {
            return Err(Error::AlreadyClaimed);
        }

        let payout = if round.is_push {
            // Refund wager
            bet.wager
        } else if bet.direction == round.outcome {
            // Winner: proportional share of net pool
            round
                .net_pool
                .checked_mul(bet.wager)
                .and_then(|v| v.checked_div(round.winning_total))
                .ok_or(Error::Overflow)?
        } else {
            0i128
        };

        if payout == 0 {
            return Err(Error::NoPayout);
        }

        // State update before transfer (reentrancy-safe)
        bet.claimed = true;
        env.storage().persistent().set(&bet_key, &bet);
        env.storage()
            .persistent()
            .extend_ttl(&bet_key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);

        let token = get_token(&env);
        TokenClient::new(&env, &token).transfer(
            &env.current_contract_address(),
            &player,
            &payout,
        );

        Claimed { round_id, player, payout }.publish(&env);
        Ok(())
    }

    /// View a round's state.
    pub fn get_round(env: Env, round_id: u64) -> Result<RoundData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Round(round_id))
            .ok_or(Error::RoundNotFound)
    }

    /// View a player's bet in a round.
    pub fn get_bet(env: Env, round_id: u64, player: Address) -> Result<BetData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Bet(BetKey { round_id, player }))
            .ok_or(Error::BetNotFound)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn require_initialized(env: &Env) -> Result<(), Error> {
    if !env.storage().instance().has(&DataKey::Admin) {
        return Err(Error::NotInitialized);
    }
    Ok(())
}

fn require_admin(env: &Env) -> Result<(), Error> {
    let admin: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)?;
    admin.require_auth();
    Ok(())
}

fn get_token(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::Token)
        .expect("PricePrediction: token not set")
}

fn get_oracle(env: &Env) -> Address {
    env.storage()
        .instance()
        .get(&DataKey::OracleContract)
        .expect("PricePrediction: oracle not set")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test;
