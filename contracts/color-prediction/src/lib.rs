//! Stellarcade Color Prediction Game Contract
//!
//! A prediction game where players wager on which color will be chosen next.
//! An admin resolves each game by revealing the winning color. Winners split
//! the pot proportionally; losers forfeit their wager to the pool.
//!
//! ## Game Flow
//! 1. Admin calls `init` to configure the contract.
//! 2. Player calls `place_prediction(player, color, wager, game_id)` to enter.
//!    Multiple players can predict on the same game_id. Each player may only
//!    submit one prediction per game.
//! 3. Admin calls `resolve_prediction(game_id)` with the winning color.
//!    Winners are determined and the pot split equally among correct predictors.
//! 4. Anyone calls `get_game(game_id)` to inspect the final state.
//!
//! ## Colors
//! Valid color values: 0 = Red, 1 = Green, 2 = Blue, 3 = Yellow.
//!
//! ## Storage Strategy
//! - `instance()` storage: contract-level config (Admin, RngContract,
//!   PrizePoolContract, BalanceContract). Small, bounded, single ledger entry.
//! - `persistent()` storage: per-game and per-player data (GameData,
//!   PlayerList, Prediction). Each is an independent ledger entry with its own
//!   TTL extended on every write (~30 days).
//!
//! ## Security
//! - Only admin may resolve predictions.
//! - Each player may predict at most once per game.
//! - Resolving an already-resolved game is rejected.
//! - All arithmetic uses `checked_*` to prevent overflow.
#![no_std]
#![allow(unexpected_cfgs)]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Vec,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Persistent storage TTL in ledgers (~30 days at 5 s/ledger).
pub const PERSISTENT_BUMP_LEDGERS: u32 = 518_400;

/// Maximum number of players per game (bounds O(n) iteration in resolve).
pub const MAX_PLAYERS_PER_GAME: u32 = 500;

// ---------------------------------------------------------------------------
// Color constants
// ---------------------------------------------------------------------------

pub const COLOR_RED: u32 = 0;
pub const COLOR_GREEN: u32 = 1;
pub const COLOR_BLUE: u32 = 2;
pub const COLOR_YELLOW: u32 = 3;
pub const COLOR_MAX: u32 = COLOR_YELLOW;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    NotAuthorized = 3,
    InvalidColor = 4,
    InvalidAmount = 5,
    GameNotFound = 6,
    GameAlreadyResolved = 7,
    AlreadyPredicted = 8,
    GameFull = 9,
    Overflow = 10,
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// Lifecycle state of a prediction game.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameStatus {
    /// Accepting predictions.
    Open = 0,
    /// Resolved — winning color known, outcome recorded.
    Resolved = 1,
}

/// Metadata and accumulated state for one prediction game.
#[contracttype]
#[derive(Clone)]
pub struct GameData {
    /// Total tokens wagered across all predictions.
    pub total_pot: i128,
    /// Number of distinct predictors.
    pub player_count: u32,
    /// Number of predictors who chose the winning color.
    pub winner_count: u32,
    /// Winning color (only valid when status == Resolved).
    pub winning_color: u32,
    pub status: GameStatus,
}

/// A single player's prediction for a game.
#[contracttype]
#[derive(Clone)]
pub struct PredictionEntry {
    pub color: u32,
    pub wager: i128,
}

/// Storage key discriminants.
///
/// Instance keys (Admin, RngContract, PrizePoolContract, BalanceContract)
/// hold small contract-level config in a single ledger entry.
///
/// Persistent keys (Game, PlayerList, Prediction) are per-game and per-player,
/// each stored as an independent ledger entry with its own TTL.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    // --- instance() keys ---
    Admin,
    RngContract,
    PrizePoolContract,
    BalanceContract,
    // --- persistent() keys ---
    /// GameData keyed by game_id.
    Game(u64),
    /// Vec<Address> of all predictors for a game.
    PlayerList(u64),
    /// PredictionEntry keyed by (game_id, player).
    Prediction(u64, Address),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct PredictionPlaced {
    #[topic]
    pub game_id: u64,
    #[topic]
    pub player: Address,
    pub color: u32,
    pub wager: i128,
}

#[contractevent]
pub struct PredictionResolved {
    #[topic]
    pub game_id: u64,
    pub winning_color: u32,
    pub winner_count: u32,
    pub total_pot: i128,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ColorPrediction;

#[contractimpl]
impl ColorPrediction {
    // -----------------------------------------------------------------------
    // init
    // -----------------------------------------------------------------------

    /// Initialize the contract. May only be called once.
    ///
    /// Stores admin, rng_contract, prize_pool_contract, and balance_contract
    /// in instance storage. Subsequent calls return `AlreadyInitialized`.
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

    // -----------------------------------------------------------------------
    // place_prediction
    // -----------------------------------------------------------------------

    /// Place a color prediction for an open game.
    ///
    /// `color` must be one of COLOR_RED (0), COLOR_GREEN (1), COLOR_BLUE (2),
    /// COLOR_YELLOW (3). `wager` must be positive. Each player may predict
    /// exactly once per game. The game is created implicitly on the first
    /// prediction for a given `game_id`.
    ///
    /// Emits `PredictionPlaced`.
    pub fn place_prediction(
        env: Env,
        player: Address,
        color: u32,
        wager: i128,
        game_id: u64,
    ) -> Result<(), Error> {
        require_initialized(&env)?;
        player.require_auth();

        if color > COLOR_MAX {
            return Err(Error::InvalidColor);
        }
        if wager <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Load or initialize the game.
        let mut game: GameData = env
            .storage()
            .persistent()
            .get(&DataKey::Game(game_id))
            .unwrap_or(GameData {
                total_pot: 0,
                player_count: 0,
                winner_count: 0,
                winning_color: 0,
                status: GameStatus::Open,
            });

        if game.status != GameStatus::Open {
            return Err(Error::GameAlreadyResolved);
        }

        if game.player_count >= MAX_PLAYERS_PER_GAME {
            return Err(Error::GameFull);
        }

        let prediction_key = DataKey::Prediction(game_id, player.clone());
        if env.storage().persistent().has(&prediction_key) {
            return Err(Error::AlreadyPredicted);
        }

        // Record the prediction.
        let entry = PredictionEntry { color, wager };
        persist_set(&env, prediction_key, &entry);

        // Register player in the list.
        let mut players: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerList(game_id))
            .unwrap_or_else(|| Vec::new(&env));
        players.push_back(player.clone());
        persist_set(&env, DataKey::PlayerList(game_id), &players);

        // Update game totals.
        game.total_pot = game.total_pot.checked_add(wager).ok_or(Error::Overflow)?;
        game.player_count = game.player_count.checked_add(1).ok_or(Error::Overflow)?;
        persist_set(&env, DataKey::Game(game_id), &game);

        // TODO: Invoke balance_contract to transfer `wager` tokens from player to this contract.

        PredictionPlaced {
            game_id,
            player,
            color,
            wager,
        }
        .publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // resolve_prediction
    // -----------------------------------------------------------------------

    /// Resolve a game by declaring the winning color. Admin only.
    ///
    /// `winning_color` must be a valid color value (0–3). Iterates all player
    /// predictions (bounded by `MAX_PLAYERS_PER_GAME`) to count winners and
    /// transitions the game to `Resolved`.
    ///
    /// If there are no winners, the entire pot remains in the contract.
    ///
    /// Emits `PredictionResolved`.
    pub fn resolve_prediction(env: Env, game_id: u64, winning_color: u32) -> Result<(), Error> {
        let admin = get_admin(&env)?;
        admin.require_auth();

        if winning_color > COLOR_MAX {
            return Err(Error::InvalidColor);
        }

        let mut game: GameData = env
            .storage()
            .persistent()
            .get(&DataKey::Game(game_id))
            .ok_or(Error::GameNotFound)?;

        if game.status != GameStatus::Open {
            return Err(Error::GameAlreadyResolved);
        }

        let players: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::PlayerList(game_id))
            .unwrap_or_else(|| Vec::new(&env));

        let mut winner_count: u32 = 0;

        // Count winners (bounded by MAX_PLAYERS_PER_GAME).
        for player in players.iter() {
            let key = DataKey::Prediction(game_id, player.clone());
            if let Some(entry) = env
                .storage()
                .persistent()
                .get::<DataKey, PredictionEntry>(&key)
            {
                if entry.color == winning_color {
                    winner_count = winner_count.checked_add(1).ok_or(Error::Overflow)?;
                }
            }
        }

        // TODO: If winner_count > 0, invoke prize_pool_contract to distribute
        // (game.total_pot / winner_count) tokens to each winner.

        game.status = GameStatus::Resolved;
        game.winning_color = winning_color;
        game.winner_count = winner_count;
        persist_set(&env, DataKey::Game(game_id), &game);

        PredictionResolved {
            game_id,
            winning_color,
            winner_count,
            total_pot: game.total_pot,
        }
        .publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // get_game
    // -----------------------------------------------------------------------

    /// Return the game state, or `None` if the game does not exist.
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

fn get_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

/// Persist a value in persistent storage and extend its TTL.
fn persist_set<V: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(env: &Env, key: DataKey, val: &V) {
    env.storage().persistent().set(&key, val);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    fn setup(
        env: &Env,
    ) -> (
        ColorPredictionClient<'_>,
        Address,
        Address,
        Address,
        Address,
    ) {
        let id = env.register(ColorPrediction, ());
        let client = ColorPredictionClient::new(env, &id);
        let admin = Address::generate(env);
        let rng = Address::generate(env);
        let prize_pool = Address::generate(env);
        let balance = Address::generate(env);
        env.mock_all_auths();
        client.init(&admin, &rng, &prize_pool, &balance);
        (client, admin, rng, prize_pool, balance)
    }

    // ------------------------------------------------------------------
    // 1. Happy path: place predictions → resolve → inspect state
    // ------------------------------------------------------------------

    #[test]
    fn test_full_happy_path() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 1;
        let winner = Address::generate(&env);
        let loser = Address::generate(&env);

        client.place_prediction(&winner, &COLOR_RED, &100i128, &game_id);
        client.place_prediction(&loser, &COLOR_BLUE, &100i128, &game_id);

        client.resolve_prediction(&game_id, &COLOR_RED);

        let game = client.get_game(&game_id).unwrap();
        assert_eq!(game.status, GameStatus::Resolved);
        assert_eq!(game.winning_color, COLOR_RED);
        assert_eq!(game.winner_count, 1);
        assert_eq!(game.total_pot, 200);
        assert_eq!(game.player_count, 2);
    }

    // ------------------------------------------------------------------
    // 2. All players win when all predict the correct color
    // ------------------------------------------------------------------

    #[test]
    fn test_all_winners() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 2;
        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);
        let p3 = Address::generate(&env);

        client.place_prediction(&p1, &COLOR_GREEN, &50i128, &game_id);
        client.place_prediction(&p2, &COLOR_GREEN, &50i128, &game_id);
        client.place_prediction(&p3, &COLOR_GREEN, &50i128, &game_id);

        client.resolve_prediction(&game_id, &COLOR_GREEN);

        let game = client.get_game(&game_id).unwrap();
        assert_eq!(game.winner_count, 3);
        assert_eq!(game.total_pot, 150);
    }

    // ------------------------------------------------------------------
    // 3. No winners when all predict the wrong color
    // ------------------------------------------------------------------

    #[test]
    fn test_no_winners() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 3;
        let player = Address::generate(&env);
        client.place_prediction(&player, &COLOR_RED, &200i128, &game_id);

        client.resolve_prediction(&game_id, &COLOR_BLUE);

        let game = client.get_game(&game_id).unwrap();
        assert_eq!(game.winner_count, 0);
        assert_eq!(game.status, GameStatus::Resolved);
    }

    // ------------------------------------------------------------------
    // 4. Duplicate prediction rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_duplicate_prediction_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 4;
        let player = Address::generate(&env);
        client.place_prediction(&player, &COLOR_RED, &100i128, &game_id);

        let result = client.try_place_prediction(&player, &COLOR_GREEN, &100i128, &game_id);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 5. Prediction on a resolved game rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_predict_on_resolved_game_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 5;
        let p1 = Address::generate(&env);
        client.place_prediction(&p1, &COLOR_RED, &100i128, &game_id);
        client.resolve_prediction(&game_id, &COLOR_RED);

        let late = Address::generate(&env);
        let result = client.try_place_prediction(&late, &COLOR_RED, &100i128, &game_id);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 6. Double resolve rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_double_resolve_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 6;
        let player = Address::generate(&env);
        client.place_prediction(&player, &COLOR_YELLOW, &10i128, &game_id);
        client.resolve_prediction(&game_id, &COLOR_YELLOW);

        let result = client.try_resolve_prediction(&game_id, &COLOR_YELLOW);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 7. Invalid color rejected on place_prediction
    // ------------------------------------------------------------------

    #[test]
    fn test_invalid_color_on_place_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 7;
        let player = Address::generate(&env);
        let result = client.try_place_prediction(&player, &99u32, &100i128, &game_id);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 8. Invalid color rejected on resolve_prediction
    // ------------------------------------------------------------------

    #[test]
    fn test_invalid_color_on_resolve_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 8;
        let player = Address::generate(&env);
        client.place_prediction(&player, &COLOR_RED, &100i128, &game_id);

        let result = client.try_resolve_prediction(&game_id, &99u32);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 9. Zero wager rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_zero_wager_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 9;
        let player = Address::generate(&env);
        let result = client.try_place_prediction(&player, &COLOR_RED, &0i128, &game_id);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 10. Negative wager rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_negative_wager_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let game_id: u64 = 10;
        let player = Address::generate(&env);
        let result = client.try_place_prediction(&player, &COLOR_RED, &-50i128, &game_id);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 11. Non-admin cannot resolve
    // ------------------------------------------------------------------

    #[test]
    fn test_non_admin_cannot_resolve() {
        let env = Env::default();
        let (client, admin, rng, prize_pool, balance) = setup(&env);

        let id2 = env.register(ColorPrediction, ());
        let client2 = ColorPredictionClient::new(&env, &id2);
        env.mock_all_auths();
        client2.init(&admin, &rng, &prize_pool, &balance);

        let game_id: u64 = 11;
        let player = Address::generate(&env);
        client2.place_prediction(&player, &COLOR_RED, &100i128, &game_id);

        let imposter = Address::generate(&env);
        env.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &imposter,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &id2,
                fn_name: "resolve_prediction",
                args: soroban_sdk::vec![
                    &env,
                    soroban_sdk::IntoVal::into_val(&game_id, &env),
                    soroban_sdk::IntoVal::into_val(&COLOR_RED, &env),
                ],
                sub_invokes: &[],
            },
        }]);

        let result = client2.try_resolve_prediction(&game_id, &COLOR_RED);
        assert!(result.is_err());

        let _ = client;
    }

    // ------------------------------------------------------------------
    // 12. Cannot initialize twice
    // ------------------------------------------------------------------

    #[test]
    fn test_cannot_init_twice() {
        let env = Env::default();
        let (client, admin, rng, prize_pool, balance) = setup(&env);
        env.mock_all_auths();

        let result = client.try_init(&admin, &rng, &prize_pool, &balance);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 13. Resolve non-existent game rejected
    // ------------------------------------------------------------------

    #[test]
    fn test_resolve_nonexistent_game_rejected() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let result = client.try_resolve_prediction(&999u64, &COLOR_RED);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // 14. get_game returns None for unknown game
    // ------------------------------------------------------------------

    #[test]
    fn test_get_game_none_for_unknown() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let result = client.get_game(&9999u64);
        assert!(result.is_none());
    }

    // ------------------------------------------------------------------
    // 15. Multiple games are independent
    // ------------------------------------------------------------------

    #[test]
    fn test_multiple_games_independent() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        let p1 = Address::generate(&env);
        let p2 = Address::generate(&env);

        client.place_prediction(&p1, &COLOR_RED, &100i128, &1u64);
        client.place_prediction(&p2, &COLOR_BLUE, &200i128, &2u64);

        client.resolve_prediction(&1u64, &COLOR_RED);
        client.resolve_prediction(&2u64, &COLOR_GREEN);

        let game1 = client.get_game(&1u64).unwrap();
        let game2 = client.get_game(&2u64).unwrap();

        assert_eq!(game1.winner_count, 1);
        assert_eq!(game1.total_pot, 100);
        assert_eq!(game2.winner_count, 0);
        assert_eq!(game2.total_pot, 200);
    }

    // ------------------------------------------------------------------
    // 16. All four valid colors can be used
    // ------------------------------------------------------------------

    #[test]
    fn test_all_valid_colors_accepted() {
        let env = Env::default();
        let (client, _, _, _, _) = setup(&env);
        env.mock_all_auths();

        for (game_id, color) in [
            (20u64, COLOR_RED),
            (21u64, COLOR_GREEN),
            (22u64, COLOR_BLUE),
            (23u64, COLOR_YELLOW),
        ] {
            let player = Address::generate(&env);
            client.place_prediction(&player, &color, &10i128, &game_id);
            client.resolve_prediction(&game_id, &color);
            let game = client.get_game(&game_id).unwrap();
            assert_eq!(game.winner_count, 1);
        }
    }
}
