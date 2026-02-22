#![cfg(test)]

use super::*;
use soroban_sdk::{
    contract, contractimpl, contracttype,
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    Address, Env, Symbol,
};

// -------------------------------------------------------------------
// Mock Oracle Contract
// -------------------------------------------------------------------

#[contract]
pub struct MockOracle;

#[contracttype]
pub enum OracleKey {
    Price(Symbol),
}

#[contractimpl]
impl MockOracle {
    pub fn set_price(env: Env, asset: Symbol, price: i128) {
        env.storage()
            .persistent()
            .set(&OracleKey::Price(asset), &price);
    }

    pub fn get_price(env: Env, asset: Symbol) -> i128 {
        env.storage()
            .persistent()
            .get(&OracleKey::Price(asset))
            .unwrap_or(0)
    }
}

// -------------------------------------------------------------------
// Helpers
// -------------------------------------------------------------------

fn create_token<'a>(env: &'a Env, admin: &Address) -> (Address, StellarAssetClient<'a>) {
    let contract = env.register_stellar_asset_contract_v2(admin.clone());
    let client = StellarAssetClient::new(env, &contract.address());
    (contract.address(), client)
}

fn btc(env: &Env) -> Symbol {
    Symbol::new(env, "BTC")
}

struct Setup<'a> {
    client: PricePredictionClient<'a>,
    oracle_client: MockOracleClient<'a>,
    token_addr: Address,
    token_sac: StellarAssetClient<'a>,
}

fn setup(env: &Env) -> Setup<'_> {
    let admin = Address::generate(env);
    let token_admin = Address::generate(env);

    let (token_addr, token_sac) = create_token(env, &token_admin);

    // Deploy mock oracle
    let oracle_id = env.register(MockOracle, ());
    let oracle_client = MockOracleClient::new(env, &oracle_id);

    // Deploy PricePrediction
    let contract_id = env.register(PricePrediction, ());
    let client = PricePredictionClient::new(env, &contract_id);

    env.mock_all_auths();

    // Set initial oracle price for BTC
    oracle_client.set_price(&btc(env), &50_000);

    // Init: min=10, max=10000, house edge 500 bps (5%)
    client.init(&admin, &oracle_id, &token_addr, &10i128, &10_000i128, &500i128);

    // Fund contract for payouts
    token_sac.mint(&contract_id, &1_000_000i128);

    // Set initial ledger timestamp
    env.ledger().with_mut(|li| {
        li.timestamp = 1000;
    });

    Setup {
        client,
        oracle_client,
        token_addr,
        token_sac,
    }
}

fn tc<'a>(env: &'a Env, token: &Address) -> TokenClient<'a> {
    TokenClient::new(env, token)
}

// -------------------------------------------------------------------
// 1. Initialization
// -------------------------------------------------------------------

#[test]
fn test_init_rejects_reinit() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let oracle = Address::generate(&env);
    let tok = Address::generate(&env);
    let result = s.client.try_init(&Address::generate(&env), &oracle, &tok, &10, &10000, &500);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 2. Open market happy path
// -------------------------------------------------------------------

#[test]
fn test_open_market_happy_path() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    let round = s.client.get_round(&1u64);
    assert_eq!(round.open_price, 50_000);
    assert_eq!(round.close_time, 2000);
    assert!(!round.settled);
    assert_eq!(round.total_up, 0);
    assert_eq!(round.total_down, 0);
}

// -------------------------------------------------------------------
// 3. Open market - duplicate round rejected
// -------------------------------------------------------------------

#[test]
fn test_open_market_duplicate_round_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    let result = s.client.try_open_market(&1u64, &btc(&env), &3000u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 4. Open market - close_time in past rejected
// -------------------------------------------------------------------

#[test]
fn test_open_market_past_close_time_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    // Timestamp is 1000, close_time = 500 (in past)
    let result = s.client.try_open_market(&1u64, &btc(&env), &500u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 5. Open market - invalid oracle price rejected
// -------------------------------------------------------------------

#[test]
fn test_open_market_zero_price_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    // Set oracle price to 0 for a different asset
    let eth = Symbol::new(&env, "ETH");
    s.oracle_client.set_price(&eth, &0);

    let result = s.client.try_open_market(&1u64, &eth, &2000u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 6. Place prediction - UP happy path
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_up() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &100);

    let round = s.client.get_round(&1u64);
    assert_eq!(round.total_up, 100);
    assert_eq!(round.total_down, 0);

    let bet = s.client.get_bet(&1u64, &player);
    assert_eq!(bet.direction, DIRECTION_UP);
    assert_eq!(bet.wager, 100);
    assert!(!bet.claimed);

    // Token transferred from player
    assert_eq!(tc(&env, &s.token_addr).balance(&player), 4900);
}

// -------------------------------------------------------------------
// 7. Place prediction - DOWN happy path
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_down() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_DOWN, &200);

    let round = s.client.get_round(&1u64);
    assert_eq!(round.total_up, 0);
    assert_eq!(round.total_down, 200);
}

// -------------------------------------------------------------------
// 8. Place prediction - invalid direction rejected
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_invalid_direction() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    let result = s.client.try_place_prediction(&player, &1u64, &2u32, &100);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 9. Place prediction - wager limits
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_wager_too_low() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_UP, &5i128); // min=10
    assert!(result.is_err());
}

#[test]
fn test_place_prediction_wager_too_high() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &50_000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_UP, &10_001i128); // max=10000
    assert!(result.is_err());
}

#[test]
fn test_place_prediction_zero_wager() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.client.open_market(&1u64, &btc(&env), &2000u64);
    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_UP, &0i128);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 10. Place prediction - after close_time rejected
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_after_close_time_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    // Advance time past close
    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });

    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_UP, &100);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 11. Place prediction - duplicate rejected
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_duplicate_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &100);

    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_DOWN, &200);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 12. Place prediction - round not found
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_round_not_found() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    let result = s.client.try_place_prediction(&player, &99u64, &DIRECTION_UP, &100);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 13. Settle round - UP wins
// -------------------------------------------------------------------

#[test]
fn test_settle_round_up_wins() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    s.token_sac.mint(&player_a, &5000);
    s.token_sac.mint(&player_b, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_UP, &300);
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_DOWN, &500);

    // Advance time and set higher price
    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // Price went up

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.settled);
    assert_eq!(round.outcome, OUTCOME_UP);
    assert!(!round.is_push);
    // Total pool = 800, fee = 800 * 500 / 10000 = 40, net = 760
    assert_eq!(round.net_pool, 760);
    assert_eq!(round.winning_total, 300);
}

// -------------------------------------------------------------------
// 14. Settle round - DOWN wins
// -------------------------------------------------------------------

#[test]
fn test_settle_round_down_wins() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    s.token_sac.mint(&player_a, &5000);
    s.token_sac.mint(&player_b, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_UP, &400);
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_DOWN, &600);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &45_000); // Price went down

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.settled);
    assert_eq!(round.outcome, OUTCOME_DOWN);
    assert!(!round.is_push);
    // Total pool = 1000, fee = 50, net = 950
    assert_eq!(round.net_pool, 950);
    assert_eq!(round.winning_total, 600);
}

// -------------------------------------------------------------------
// 15. Settle round - flat (push)
// -------------------------------------------------------------------

#[test]
fn test_settle_round_flat_push() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    s.token_sac.mint(&player_a, &5000);
    s.token_sac.mint(&player_b, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_UP, &100);
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_DOWN, &200);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    // Price unchanged → flat → push
    // oracle still returns 50_000 (same as open_price)

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.settled);
    assert_eq!(round.outcome, OUTCOME_FLAT);
    assert!(round.is_push);
    assert_eq!(round.net_pool, 0);
}

// -------------------------------------------------------------------
// 16. Settle round - before close_time rejected
// -------------------------------------------------------------------

#[test]
fn test_settle_round_before_close_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    // Timestamp is 1000, close_time = 2000 → too early
    let result = s.client.try_settle_round(&1u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 17. Settle round - double settle rejected
// -------------------------------------------------------------------

#[test]
fn test_settle_round_double_settle_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);

    s.client.settle_round(&1u64);
    let result = s.client.try_settle_round(&1u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 18. Settle round - one side only (push)
// -------------------------------------------------------------------

#[test]
fn test_settle_round_one_side_only_push() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    // Only UP bets, no DOWN bets
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &500);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // Price up, but no opposition

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.settled);
    assert!(round.is_push); // Push because only one side
}

// -------------------------------------------------------------------
// 19. Settle round - no bets (push)
// -------------------------------------------------------------------

#[test]
fn test_settle_round_no_bets_push() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.settled);
    assert!(round.is_push);
}

// -------------------------------------------------------------------
// 20. Claim - winner
// -------------------------------------------------------------------

#[test]
fn test_claim_winner() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let winner = Address::generate(&env);
    let loser = Address::generate(&env);
    s.token_sac.mint(&winner, &5000);
    s.token_sac.mint(&loser, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&winner, &1u64, &DIRECTION_UP, &300);
    s.client.place_prediction(&loser, &1u64, &DIRECTION_DOWN, &700);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // UP wins

    s.client.settle_round(&1u64);
    s.client.claim(&winner, &1u64);

    // Total pool = 1000, fee = 50, net = 950
    // Winner's share = 950 * 300 / 300 = 950 (sole UP bettor)
    let bet = s.client.get_bet(&1u64, &winner);
    assert!(bet.claimed);
    assert_eq!(tc(&env, &s.token_addr).balance(&winner), 5000 - 300 + 950);
}

// -------------------------------------------------------------------
// 21. Claim - push refund
// -------------------------------------------------------------------

#[test]
fn test_claim_push_refund() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &400);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    // Price unchanged → flat → push

    s.client.settle_round(&1u64);
    s.client.claim(&player, &1u64);

    // Full refund
    assert_eq!(tc(&env, &s.token_addr).balance(&player), 5000);
}

// -------------------------------------------------------------------
// 22. Claim - loser rejected
// -------------------------------------------------------------------

#[test]
fn test_claim_loser_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let winner = Address::generate(&env);
    let loser = Address::generate(&env);
    s.token_sac.mint(&winner, &5000);
    s.token_sac.mint(&loser, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&winner, &1u64, &DIRECTION_UP, &300);
    s.client.place_prediction(&loser, &1u64, &DIRECTION_DOWN, &700);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // UP wins

    s.client.settle_round(&1u64);

    let result = s.client.try_claim(&loser, &1u64);
    assert!(result.is_err()); // NoPayout
}

// -------------------------------------------------------------------
// 23. Claim - double claim rejected
// -------------------------------------------------------------------

#[test]
fn test_claim_double_claim_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    s.token_sac.mint(&player_a, &5000);
    s.token_sac.mint(&player_b, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_UP, &500);
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_DOWN, &500);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);

    s.client.settle_round(&1u64);
    s.client.claim(&player_a, &1u64);

    let result = s.client.try_claim(&player_a, &1u64);
    assert!(result.is_err()); // AlreadyClaimed
}

// -------------------------------------------------------------------
// 24. Claim - not settled rejected
// -------------------------------------------------------------------

#[test]
fn test_claim_not_settled_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &100);

    let result = s.client.try_claim(&player, &1u64);
    assert!(result.is_err()); // NotSettled
}

// -------------------------------------------------------------------
// 25. Claim - bet not found rejected
// -------------------------------------------------------------------

#[test]
fn test_claim_bet_not_found() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &100);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);
    s.client.settle_round(&1u64);

    // Different player who didn't bet
    let stranger = Address::generate(&env);
    let result = s.client.try_claim(&stranger, &1u64);
    assert!(result.is_err()); // BetNotFound
}

// -------------------------------------------------------------------
// 26. Multiple players - proportional payout
// -------------------------------------------------------------------

#[test]
fn test_multiple_players_proportional_payout() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    let player_c = Address::generate(&env);
    s.token_sac.mint(&player_a, &10_000);
    s.token_sac.mint(&player_b, &10_000);
    s.token_sac.mint(&player_c, &10_000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    // Two UP bettors, one DOWN bettor
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_UP, &300);   // UP
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_UP, &200);   // UP
    s.client.place_prediction(&player_c, &1u64, &DIRECTION_DOWN, &500); // DOWN

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // UP wins

    s.client.settle_round(&1u64);

    // Total pool = 1000, fee = 50, net = 950
    // winning_total = 500 (total_up)
    // Player A payout: 950 * 300 / 500 = 570
    // Player B payout: 950 * 200 / 500 = 380

    s.client.claim(&player_a, &1u64);
    s.client.claim(&player_b, &1u64);

    assert_eq!(tc(&env, &s.token_addr).balance(&player_a), 10_000 - 300 + 570);
    assert_eq!(tc(&env, &s.token_addr).balance(&player_b), 10_000 - 200 + 380);
    // Player C lost, no claim — balance stays at 10_000 - 500
    assert_eq!(tc(&env, &s.token_addr).balance(&player_c), 9_500);
}

// -------------------------------------------------------------------
// 27. Full lifecycle - multiple rounds
// -------------------------------------------------------------------

#[test]
fn test_full_lifecycle_two_rounds() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &10_000);

    // Round 1: player bets UP, price goes up → wins
    s.client.open_market(&1u64, &btc(&env), &2000u64);
    s.client.place_prediction(&player, &1u64, &DIRECTION_UP, &100);

    // Need a second player on the other side for non-push
    let opponent = Address::generate(&env);
    s.token_sac.mint(&opponent, &10_000);
    s.client.place_prediction(&opponent, &1u64, &DIRECTION_DOWN, &100);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);
    s.client.settle_round(&1u64);
    s.client.claim(&player, &1u64);

    let r1 = s.client.get_round(&1u64);
    assert!(r1.settled);
    assert_eq!(r1.outcome, OUTCOME_UP);

    // Round 2: player bets DOWN, price goes down → wins
    s.oracle_client.set_price(&btc(&env), &60_000); // new open price
    s.client.open_market(&2u64, &btc(&env), &5000u64);
    s.client.place_prediction(&player, &2u64, &DIRECTION_DOWN, &100);
    s.client.place_prediction(&opponent, &2u64, &DIRECTION_UP, &100);

    env.ledger().with_mut(|li| {
        li.timestamp = 6000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000); // Price went down
    s.client.settle_round(&2u64);
    s.client.claim(&player, &2u64);

    let r2 = s.client.get_round(&2u64);
    assert!(r2.settled);
    assert_eq!(r2.outcome, OUTCOME_DOWN);
}

// -------------------------------------------------------------------
// 28. Push round - one side only, players get refund
// -------------------------------------------------------------------

#[test]
fn test_push_one_side_refund() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player_a = Address::generate(&env);
    let player_b = Address::generate(&env);
    s.token_sac.mint(&player_a, &5000);
    s.token_sac.mint(&player_b, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);
    // Both bet DOWN, no UP bets
    s.client.place_prediction(&player_a, &1u64, &DIRECTION_DOWN, &300);
    s.client.place_prediction(&player_b, &1u64, &DIRECTION_DOWN, &200);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &45_000); // Price down, but push (no opposition)

    s.client.settle_round(&1u64);

    let round = s.client.get_round(&1u64);
    assert!(round.is_push);

    // Both get full refund
    s.client.claim(&player_a, &1u64);
    s.client.claim(&player_b, &1u64);

    assert_eq!(tc(&env, &s.token_addr).balance(&player_a), 5000);
    assert_eq!(tc(&env, &s.token_addr).balance(&player_b), 5000);
}

// -------------------------------------------------------------------
// 29. Get round - not found
// -------------------------------------------------------------------

#[test]
fn test_get_round_not_found() {
    let env = Env::default();
    let s = setup(&env);

    let result = s.client.try_get_round(&99u64);
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// 30. Place prediction on settled round rejected
// -------------------------------------------------------------------

#[test]
fn test_place_prediction_on_settled_round_rejected() {
    let env = Env::default();
    let s = setup(&env);
    env.mock_all_auths();

    let player = Address::generate(&env);
    s.token_sac.mint(&player, &5000);

    s.client.open_market(&1u64, &btc(&env), &2000u64);

    env.ledger().with_mut(|li| {
        li.timestamp = 3000;
    });
    s.oracle_client.set_price(&btc(&env), &55_000);
    s.client.settle_round(&1u64);

    let result = s.client.try_place_prediction(&player, &1u64, &DIRECTION_UP, &100);
    assert!(result.is_err());
}
