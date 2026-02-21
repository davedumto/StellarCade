//! Stellarcade Higher or Lower Contract
//!
//! A simple prediction game: players wager on whether the outcome is higher
//! or lower than a fixed anchor value.
#![no_std]
#![allow(unexpected_cfgs)]

use soroban_sdk::{
    contract, contractclient, contracterror, contractevent, contractimpl, contracttype,
    symbol_short, Address, Env, Symbol,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

pub const MIN_WAGER: i128 = 1;
pub const MAX_WAGER: i128 = 1_000_000_000;
pub const ANCHOR_VALUE: u32 = 50;

// ---------------------------------------------------------------------------
// External contract clients
// ---------------------------------------------------------------------------

#[contractclient(name = "RngClient")]
pub trait RngContract {
    fn is_ready(env: Env, game_id: u64) -> bool;
    fn get_result(env: Env, game_id: u64) -> u32;
}

#[contractclient(name = "BalanceClient")]
pub trait UserBalanceContract {
    fn debit(env: Env, game: Address, user: Address, amount: i128, reason: Symbol);
    fn credit(env: Env, game: Address, user: Address, amount: i128, reason: Symbol);
    fn balance_of(env: Env, user: Address) -> i128;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    InvalidPrediction = 4,
    InvalidWager = 5,
    GameAlreadyExists = 6,
    GameNotFound = 7,
    AlreadyResolved = 8,
    RngNotReady = 9,
    InsufficientBalance = 10,
    HouseInsufficientFunds = 11,
    Overflow = 12,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prediction {
    Higher = 0,
    Lower = 1,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GameData {
    pub player: Address,
    pub prediction: Prediction,
    pub wager: i128,
    pub resolved: bool,
    pub outcome: u32,
    pub win: bool,
    pub payout: i128,
}

#[contracttype]
pub enum DataKey {
    Admin,
    RngContract,
    PrizePoolContract,
    BalanceContract,
    Game(u64),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct PredictionPlaced {
    #[topic]
    pub game_id: u64,
    pub player: Address,
    pub prediction: u32,
    pub wager: i128,
}

#[contractevent]
pub struct GameResolved {
    #[topic]
    pub game_id: u64,
    pub outcome: u32,
    pub win: bool,
    pub payout: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct HigherLower;

#[contractimpl]
impl HigherLower {
    pub fn init(
        env: Env,
        admin: Address,
        rng_contract: Address,
        prize_pool_contract: Address,
        balance_contract: Address,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::RngContract, &rng_contract);
        env.storage()
            .instance()
            .set(&DataKey::PrizePoolContract, &prize_pool_contract);
        env.storage()
            .instance()
            .set(&DataKey::BalanceContract, &balance_contract);
        Ok(())
    }

    pub fn place_prediction(
        env: Env,
        player: Address,
        prediction: u32,
        wager: i128,
        game_id: u64,
    ) -> Result<(), Error> {
        require_initialized(&env)?;
        player.require_auth();

        let prediction = parse_prediction(prediction)?;
        require_wager_bounds(wager)?;

        let key = DataKey::Game(game_id);
        if env.storage().persistent().has(&key) {
            return Err(Error::GameAlreadyExists);
        }

        let balance_contract = get_balance_contract(&env)?;
        let game_addr = env.current_contract_address();
        let balance_client = BalanceClient::new(&env, &balance_contract);

        let player_balance = balance_client.balance_of(&player);
        if player_balance < wager {
            return Err(Error::InsufficientBalance);
        }

        balance_client.debit(&game_addr, &player, &wager, &symbol_short!("wager"));
        balance_client.credit(&game_addr, &game_addr, &wager, &symbol_short!("escrow"));

        let game = GameData {
            player: player.clone(),
            prediction,
            wager,
            resolved: false,
            outcome: 0,
            win: false,
            payout: 0,
        };
        env.storage().persistent().set(&key, &game);

        PredictionPlaced {
            game_id,
            player,
            prediction: prediction as u32,
            wager,
        }
        .publish(&env);

        Ok(())
    }

    pub fn resolve_game(env: Env, game_id: u64) -> Result<(), Error> {
        require_initialized(&env)?;

        let key = DataKey::Game(game_id);
        let mut game: GameData = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::GameNotFound)?;

        if game.resolved {
            return Err(Error::AlreadyResolved);
        }

        let rng_contract = get_rng_contract(&env)?;
        let rng_client = RngClient::new(&env, &rng_contract);
        if !rng_client.is_ready(&game_id) {
            return Err(Error::RngNotReady);
        }
        let outcome = rng_client.get_result(&game_id);

        let win = match game.prediction {
            Prediction::Higher => outcome > ANCHOR_VALUE,
            Prediction::Lower => outcome < ANCHOR_VALUE,
        };

        let payout = if win {
            game.wager.checked_mul(2).ok_or(Error::Overflow)?
        } else {
            0
        };

        let balance_contract = get_balance_contract(&env)?;
        let game_addr = env.current_contract_address();
        let balance_client = BalanceClient::new(&env, &balance_contract);

        if payout > 0 {
            let house_balance = balance_client.balance_of(&game_addr);
            if house_balance < payout {
                return Err(Error::HouseInsufficientFunds);
            }

            balance_client.debit(&game_addr, &game_addr, &payout, &symbol_short!("payout"));
            balance_client.credit(&game_addr, &game.player, &payout, &symbol_short!("win"));
        }

        game.resolved = true;
        game.outcome = outcome;
        game.win = win;
        game.payout = payout;
        env.storage().persistent().set(&key, &game);

        GameResolved {
            game_id,
            outcome,
            win,
            payout,
        }
        .publish(&env);

        Ok(())
    }

    pub fn get_game(env: Env, game_id: u64) -> Option<GameData> {
        env.storage().persistent().get(&DataKey::Game(game_id))
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

fn require_wager_bounds(wager: i128) -> Result<(), Error> {
    if wager < MIN_WAGER || wager > MAX_WAGER {
        return Err(Error::InvalidWager);
    }
    Ok(())
}

fn parse_prediction(value: u32) -> Result<Prediction, Error> {
    match value {
        0 => Ok(Prediction::Higher),
        1 => Ok(Prediction::Lower),
        _ => Err(Error::InvalidPrediction),
    }
}

fn get_rng_contract(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::RngContract)
        .ok_or(Error::NotInitialized)
}

fn get_balance_contract(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::BalanceContract)
        .ok_or(Error::NotInitialized)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        contract, contractimpl, contracttype, testutils::Address as _, token::StellarAssetClient,
        Address, Env,
    };
    use stellarcade_user_balance::{UserBalance, UserBalanceClient};

    // -----------------------------
    // Mock RNG contract
    // -----------------------------

    #[contract]
    pub struct MockRng;

    #[contracttype]
    pub enum RngKey {
        Result(u64),
        Ready(u64),
    }

    #[contractimpl]
    impl MockRng {
        pub fn set_result(env: Env, game_id: u64, result: u32) {
            env.storage().persistent().set(&RngKey::Result(game_id), &result);
            env.storage().persistent().set(&RngKey::Ready(game_id), &true);
        }

        pub fn is_ready(env: Env, game_id: u64) -> bool {
            env.storage()
                .persistent()
                .get(&RngKey::Ready(game_id))
                .unwrap_or(false)
        }

        pub fn get_result(env: Env, game_id: u64) -> u32 {
            env.storage()
                .persistent()
                .get(&RngKey::Result(game_id))
                .unwrap_or(0)
        }
    }

    fn create_token<'a>(env: &'a Env, token_admin: &Address) -> (Address, StellarAssetClient<'a>) {
        let contract = env.register_stellar_asset_contract_v2(token_admin.clone());
        let client = StellarAssetClient::new(env, &contract.address());
        (contract.address(), client)
    }

    fn setup(
        env: &Env,
    ) -> (
        HigherLowerClient<'_>,
        Address, // admin
        Address, // player
        Address, // house
        UserBalanceClient<'_>,
        MockRngClient<'_>,
    ) {
        env.mock_all_auths();

        let admin = Address::generate(env);
        let player = Address::generate(env);
        let token_admin = Address::generate(env);

        let (token_addr, token_sac) = create_token(env, &token_admin);

        let balance_id = env.register(UserBalance, ());
        let balance_client = UserBalanceClient::new(env, &balance_id);
        balance_client.init(&admin, &token_addr);

        let rng_id = env.register(MockRng, ());
        let rng_client = MockRngClient::new(env, &rng_id);

        let higher_lower_id = env.register(HigherLower, ());
        let higher_lower_client = HigherLowerClient::new(env, &higher_lower_id);

        let house = higher_lower_id.clone();

        higher_lower_client.init(&admin, &rng_id, &Address::generate(env), &balance_id);

        balance_client.authorize_game(&admin, &higher_lower_id);

        token_sac.mint(&player, &1_000);
        token_sac.mint(&house, &5_000);

        balance_client.deposit(&player, &1_000);
        balance_client.deposit(&house, &5_000);

        (
            higher_lower_client,
            admin,
            player,
            house,
            balance_client,
            rng_client,
        )
    }

    #[test]
    fn test_place_prediction_happy_path() {
        let env = Env::default();
        let (client, _admin, player, house, balance, _rng) = setup(&env);

        client.place_prediction(&player, &0, &100, &1);

        let game = client.get_game(&1).unwrap();
        assert_eq!(game.player, player);
        assert_eq!(game.prediction, Prediction::Higher);
        assert_eq!(game.wager, 100);
        assert!(!game.resolved);

        assert_eq!(balance.balance_of(&player), 900);
        assert_eq!(balance.balance_of(&house), 5_100);
    }

    #[test]
    fn test_win_resolution_path() {
        let env = Env::default();
        let (client, _admin, player, house, balance, rng) = setup(&env);

        client.place_prediction(&player, &0, &100, &2);

        rng.set_result(&2, &80);
        client.resolve_game(&2);

        let game = client.get_game(&2).unwrap();
        assert!(game.resolved);
        assert!(game.win);
        assert_eq!(game.payout, 200);

        assert_eq!(balance.balance_of(&player), 1_100);
        assert_eq!(balance.balance_of(&house), 4_900);
    }

    #[test]
    fn test_loss_resolution_path() {
        let env = Env::default();
        let (client, _admin, player, house, balance, rng) = setup(&env);

        client.place_prediction(&player, &0, &100, &3);

        rng.set_result(&3, &20);
        client.resolve_game(&3);

        let game = client.get_game(&3).unwrap();
        assert!(game.resolved);
        assert!(!game.win);
        assert_eq!(game.payout, 0);

        assert_eq!(balance.balance_of(&player), 900);
        assert_eq!(balance.balance_of(&house), 5_100);
    }

    #[test]
    fn test_invalid_prediction_rejected() {
        let env = Env::default();
        let (client, _admin, player, _house, _balance, _rng) = setup(&env);

        let result = client.try_place_prediction(&player, &2, &100, &4);
        assert!(result.is_err());
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let env = Env::default();
        let (client, _admin, player, _house, balance, _rng) = setup(&env);

        balance.withdraw(&player, &1_000);

        let result = client.try_place_prediction(&player, &0, &100, &5);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_and_double_resolution_blocked() {
        let env = Env::default();
        let (client, _admin, player, _house, _balance, rng) = setup(&env);

        client.place_prediction(&player, &1, &100, &6);
        let dup = client.try_place_prediction(&player, &1, &100, &6);
        assert!(dup.is_err());

        rng.set_result(&6, &20);
        client.resolve_game(&6);
        let again = client.try_resolve_game(&6);
        assert!(again.is_err());
    }

    #[test]
    fn test_resolve_before_rng_ready_rejected() {
        let env = Env::default();
        let (client, _admin, player, _house, _balance, _rng) = setup(&env);

        client.place_prediction(&player, &1, &100, &7);
        let result = client.try_resolve_game(&7);
        assert!(result.is_err());
    }
}
