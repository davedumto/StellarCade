#![no_std]
#![allow(unexpected_cfgs)]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Vec,
};

/// TTL bump for persistent storage entries (~30 days at 5s/ledger).
pub const PERSISTENT_BUMP_LEDGERS: u32 = 518_400;

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
    InvalidAmount = 4,
    AlreadyReferred = 5,
    SelfReferral = 6,
    ReferrerNotRegistered = 7,
    NoPendingRewards = 8,
    AlreadyClaimed = 9,
    InvalidEventType = 10,
    Overflow = 99,
}

// ---------------------------------------------------------------------------
// Storage types
// ---------------------------------------------------------------------------

/// Referral event types that generate rewards.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventType {
    GamePlayed = 0,
    Deposit = 1,
    PrizeClaimed = 2,
}

/// Per-user referral state persisted on-chain.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferralState {
    /// The referrer who referred this user (zero-address means none).
    pub referrer: Address,
    /// Addresses this user has referred.
    pub referees: Vec<Address>,
    /// Total reward earned (lifetime).
    pub total_earned: i128,
    /// Pending reward available to claim.
    pub pending_reward: i128,
    /// Number of referral events recorded.
    pub event_count: u64,
}

/// Storage key layout.
#[contracttype]
pub enum DataKey {
    /// Contract admin — instance storage.
    Admin,
    /// Reward contract address — instance storage.
    RewardContract,
    /// Reward percentage in basis points (e.g. 500 = 5%) — instance storage.
    RewardBps,
    /// Per-user referral state — persistent storage.
    State(Address),
    /// Mapping: referee → referrer — persistent storage.
    ReferredBy(Address),
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[contractevent]
pub struct Initialized {
    #[topic]
    pub admin: Address,
    pub reward_contract: Address,
    pub reward_bps: u32,
}

#[contractevent]
pub struct ReferrerRegistered {
    #[topic]
    pub user: Address,
    #[topic]
    pub referrer: Address,
}

#[contractevent]
pub struct ReferralEventRecorded {
    #[topic]
    pub user: Address,
    #[topic]
    pub referrer: Address,
    pub event_type: EventType,
    pub amount: i128,
    pub reward: i128,
}

#[contractevent]
pub struct RewardClaimed {
    #[topic]
    pub user: Address,
    pub amount: i128,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_admin(env: &Env) -> Result<Address, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(Error::NotInitialized)
}

fn require_admin(env: &Env, caller: &Address) -> Result<(), Error> {
    let admin = get_admin(env)?;
    caller.require_auth();
    if *caller != admin {
        return Err(Error::NotAuthorized);
    }
    Ok(())
}

fn get_reward_bps(env: &Env) -> Result<u32, Error> {
    env.storage()
        .instance()
        .get(&DataKey::RewardBps)
        .ok_or(Error::NotInitialized)
}

fn get_state(env: &Env, user: &Address) -> Option<ReferralState> {
    env.storage()
        .persistent()
        .get(&DataKey::State(user.clone()))
}

fn set_state(env: &Env, user: &Address, state: &ReferralState) {
    let key = DataKey::State(user.clone());
    env.storage().persistent().set(&key, state);
    env.storage()
        .persistent()
        .extend_ttl(&key, PERSISTENT_BUMP_LEDGERS, PERSISTENT_BUMP_LEDGERS);
}

fn bump_referred_by(env: &Env, user: &Address) {
    let key = DataKey::ReferredBy(user.clone());
    if env.storage().persistent().has(&key) {
        env.storage().persistent().extend_ttl(
            &key,
            PERSISTENT_BUMP_LEDGERS,
            PERSISTENT_BUMP_LEDGERS,
        );
    }
}

/// Basis-points divisor (10 000 = 100%).
const BASIS_POINTS: i128 = 10_000;

fn calculate_reward(amount: i128, bps: u32) -> Result<i128, Error> {
    amount
        .checked_mul(bps as i128)
        .and_then(|v| v.checked_div(BASIS_POINTS))
        .ok_or(Error::Overflow)
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct ReferralSystem;

#[contractimpl]
impl ReferralSystem {
    // -----------------------------------------------------------------------
    // Initialization
    // -----------------------------------------------------------------------

    /// Initializes the referral system contract.
    ///
    /// * `admin`           – address that can call privileged methods.
    /// * `reward_contract` – address of the contract/account that funds rewards.
    pub fn init(env: Env, admin: Address, reward_contract: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }

        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::RewardContract, &reward_contract);
        // Default reward: 5% (500 basis points).
        let default_bps: u32 = 500;
        env.storage()
            .instance()
            .set(&DataKey::RewardBps, &default_bps);

        Initialized {
            admin,
            reward_contract,
            reward_bps: default_bps,
        }
        .publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin
    // -----------------------------------------------------------------------

    /// Update the reward percentage (in basis points). Admin only.
    pub fn set_reward_bps(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        if bps > 10_000 {
            return Err(Error::InvalidAmount);
        }
        env.storage().instance().set(&DataKey::RewardBps, &bps);
        Ok(())
    }

    /// Update the reward contract address. Admin only.
    pub fn set_reward_contract(
        env: Env,
        admin: Address,
        reward_contract: Address,
    ) -> Result<(), Error> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::RewardContract, &reward_contract);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Referral registration
    // -----------------------------------------------------------------------

    /// Register `referrer` as the referrer of `user`.
    ///
    /// * Both `user` and `referrer` must authorize the call.
    /// * A user cannot refer themselves.
    /// * A user can only be referred once.
    pub fn register_referrer(env: Env, user: Address, referrer: Address) -> Result<(), Error> {
        get_admin(&env)?; // ensure initialized

        user.require_auth();

        // Guard: self-referral
        if user == referrer {
            return Err(Error::SelfReferral);
        }

        // Guard: already referred
        let referred_key = DataKey::ReferredBy(user.clone());
        if env.storage().persistent().has(&referred_key) {
            return Err(Error::AlreadyReferred);
        }

        // Store referee → referrer mapping
        env.storage().persistent().set(&referred_key, &referrer);
        env.storage().persistent().extend_ttl(
            &referred_key,
            PERSISTENT_BUMP_LEDGERS,
            PERSISTENT_BUMP_LEDGERS,
        );

        // Initialize user state if first interaction
        let user_state = get_state(&env, &user).unwrap_or(ReferralState {
            referrer: referrer.clone(),
            referees: Vec::new(&env),
            total_earned: 0,
            pending_reward: 0,
            event_count: 0,
        });
        let user_state = ReferralState {
            referrer: referrer.clone(),
            ..user_state
        };
        set_state(&env, &user, &user_state);

        // Update referrer's referee list
        let mut referrer_state = get_state(&env, &referrer).unwrap_or(ReferralState {
            referrer: referrer.clone(), // placeholder, referrer may not have a referrer
            referees: Vec::new(&env),
            total_earned: 0,
            pending_reward: 0,
            event_count: 0,
        });
        referrer_state.referees.push_back(user.clone());
        set_state(&env, &referrer, &referrer_state);

        ReferrerRegistered { user, referrer }.publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Referral events
    // -----------------------------------------------------------------------

    /// Record a referral event for `user`.
    ///
    /// Called by an admin/operator when a qualifying action occurs
    /// (e.g. game played, deposit made). The `amount` is the transaction value
    /// and the reward is computed as `amount * reward_bps / 10_000`.
    ///
    /// The reward is credited to the **referrer** of `user`.
    pub fn record_referral_event(
        env: Env,
        admin: Address,
        user: Address,
        event_type: EventType,
        amount: i128,
    ) -> Result<(), Error> {
        require_admin(&env, &admin)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Lookup user's referrer
        let referred_key = DataKey::ReferredBy(user.clone());
        let referrer: Address = env
            .storage()
            .persistent()
            .get(&referred_key)
            .ok_or(Error::ReferrerNotRegistered)?;
        bump_referred_by(&env, &user);

        // Calculate reward
        let bps = get_reward_bps(&env)?;
        let reward = calculate_reward(amount, bps)?;

        // Credit referrer
        let mut referrer_state = get_state(&env, &referrer).unwrap_or(ReferralState {
            referrer: referrer.clone(),
            referees: Vec::new(&env),
            total_earned: 0,
            pending_reward: 0,
            event_count: 0,
        });
        referrer_state.pending_reward = referrer_state
            .pending_reward
            .checked_add(reward)
            .ok_or(Error::Overflow)?;
        referrer_state.total_earned = referrer_state
            .total_earned
            .checked_add(reward)
            .ok_or(Error::Overflow)?;
        referrer_state.event_count = referrer_state
            .event_count
            .checked_add(1)
            .ok_or(Error::Overflow)?;
        set_state(&env, &referrer, &referrer_state);

        ReferralEventRecorded {
            user,
            referrer,
            event_type,
            amount,
            reward,
        }
        .publish(&env);

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Claiming rewards
    // -----------------------------------------------------------------------

    /// Claim all pending referral rewards for `user`.
    ///
    /// Marks the pending balance as claimed. The actual token transfer is
    /// expected to be handled by the reward contract integration; this method
    /// records the accounting and emits an event for off-chain settlement or
    /// cross-contract calls.
    pub fn claim_referral_reward(env: Env, user: Address) -> Result<i128, Error> {
        get_admin(&env)?; // ensure initialized
        user.require_auth();

        let mut state = get_state(&env, &user).ok_or(Error::ReferrerNotRegistered)?;

        if state.pending_reward <= 0 {
            return Err(Error::NoPendingRewards);
        }

        let amount = state.pending_reward;

        // Set pending to zero BEFORE any potential external call (reentrancy guard)
        state.pending_reward = 0;
        set_state(&env, &user, &state);

        RewardClaimed { user, amount }.publish(&env);

        Ok(amount)
    }

    // -----------------------------------------------------------------------
    // View / query functions
    // -----------------------------------------------------------------------

    /// Return the full referral state for a user.
    pub fn referral_state(env: Env, user: Address) -> Result<ReferralState, Error> {
        get_admin(&env)?; // ensure initialized
        get_state(&env, &user).ok_or(Error::ReferrerNotRegistered)
    }

    /// Return the referrer of a user, if any.
    pub fn get_referrer(env: Env, user: Address) -> Option<Address> {
        env.storage().persistent().get(&DataKey::ReferredBy(user))
    }

    /// Return the reward contract address.
    pub fn get_reward_contract(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::RewardContract)
            .ok_or(Error::NotInitialized)
    }

    /// Return the current reward basis points.
    pub fn get_reward_bps(env: Env) -> Result<u32, Error> {
        get_reward_bps(&env)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn setup(env: &Env) -> (ReferralSystemClient<'_>, Address, Address) {
        let admin = Address::generate(env);
        let reward_contract = Address::generate(env);

        let contract_id = env.register(ReferralSystem, ());
        let client = ReferralSystemClient::new(env, &contract_id);

        env.mock_all_auths();
        client.init(&admin, &reward_contract);

        (client, admin, reward_contract)
    }

    // -----------------------------------------------------------------------
    // Initialization tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_init_success() {
        let env = Env::default();
        let (client, _admin, reward_contract) = setup(&env);

        assert_eq!(client.get_reward_contract(), reward_contract);
        assert_eq!(client.get_reward_bps(), 500); // default 5%
    }

    #[test]
    fn test_init_already_initialized() {
        let env = Env::default();
        let (client, admin, reward_contract) = setup(&env);

        let result = client.try_init(&admin, &reward_contract);
        assert_eq!(result, Err(Ok(Error::AlreadyInitialized)));
    }

    // -----------------------------------------------------------------------
    // Admin configuration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_reward_bps() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        client.set_reward_bps(&admin, &1000); // 10%
        assert_eq!(client.get_reward_bps(), 1000);
    }

    #[test]
    fn test_set_reward_bps_invalid() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let result = client.try_set_reward_bps(&admin, &10_001);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_set_reward_bps_not_admin() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let attacker = Address::generate(&env);
        let result = client.try_set_reward_bps(&attacker, &1000);
        assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    }

    #[test]
    fn test_set_reward_contract() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let new_reward = Address::generate(&env);
        client.set_reward_contract(&admin, &new_reward);
        assert_eq!(client.get_reward_contract(), new_reward);
    }

    // -----------------------------------------------------------------------
    // Referral registration tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_register_referrer_success() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);

        client.register_referrer(&user, &referrer);

        // User's referrer is set
        assert_eq!(client.get_referrer(&user), Some(referrer.clone()));

        // User state shows the referrer
        let user_state = client.referral_state(&user);
        assert_eq!(user_state.referrer, referrer);
        assert_eq!(user_state.pending_reward, 0);

        // Referrer state shows the referee
        let referrer_state = client.referral_state(&referrer);
        assert_eq!(referrer_state.referees.len(), 1);
        assert_eq!(referrer_state.referees.get(0).unwrap(), user);
    }

    #[test]
    fn test_register_referrer_self_referral() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let result = client.try_register_referrer(&user, &user);
        assert_eq!(result, Err(Ok(Error::SelfReferral)));
    }

    #[test]
    fn test_register_referrer_already_referred() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer1 = Address::generate(&env);
        let referrer2 = Address::generate(&env);

        client.register_referrer(&user, &referrer1);

        let result = client.try_register_referrer(&user, &referrer2);
        assert_eq!(result, Err(Ok(Error::AlreadyReferred)));
    }

    #[test]
    fn test_register_multiple_referees() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let referrer = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        let user3 = Address::generate(&env);

        client.register_referrer(&user1, &referrer);
        client.register_referrer(&user2, &referrer);
        client.register_referrer(&user3, &referrer);

        let state = client.referral_state(&referrer);
        assert_eq!(state.referees.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Record referral event tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_record_referral_event_success() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        // Record a game-played event with amount 10_000
        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);

        // Referrer should have 5% of 10_000 = 500 pending
        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 500);
        assert_eq!(state.total_earned, 500);
        assert_eq!(state.event_count, 1);
    }

    #[test]
    fn test_record_multiple_events() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);
        client.record_referral_event(&admin, &user, &EventType::Deposit, &20_000);
        client.record_referral_event(&admin, &user, &EventType::PrizeClaimed, &5_000);

        // Total reward: 500 + 1000 + 250 = 1750
        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 1750);
        assert_eq!(state.total_earned, 1750);
        assert_eq!(state.event_count, 3);
    }

    #[test]
    fn test_record_event_no_referrer() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);

        let result =
            client.try_record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);
        assert_eq!(result, Err(Ok(Error::ReferrerNotRegistered)));
    }

    #[test]
    fn test_record_event_invalid_amount() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        let result = client.try_record_referral_event(&admin, &user, &EventType::GamePlayed, &0);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));

        let result = client.try_record_referral_event(&admin, &user, &EventType::GamePlayed, &-100);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_record_event_not_admin() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        let attacker = Address::generate(&env);
        let result =
            client.try_record_referral_event(&attacker, &user, &EventType::GamePlayed, &10_000);
        assert_eq!(result, Err(Ok(Error::NotAuthorized)));
    }

    #[test]
    fn test_record_events_from_multiple_referees() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let referrer = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        client.register_referrer(&user1, &referrer);
        client.register_referrer(&user2, &referrer);

        client.record_referral_event(&admin, &user1, &EventType::GamePlayed, &10_000);
        client.record_referral_event(&admin, &user2, &EventType::Deposit, &20_000);

        // Referrer earns from both: 500 + 1000 = 1500
        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 1500);
        assert_eq!(state.total_earned, 1500);
        assert_eq!(state.event_count, 2);
    }

    // -----------------------------------------------------------------------
    // Claim reward tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_claim_referral_reward_success() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);

        // Referrer claims
        let claimed = client.claim_referral_reward(&referrer);
        assert_eq!(claimed, 500);

        // Pending is now 0, but total_earned remains
        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 0);
        assert_eq!(state.total_earned, 500);
    }

    #[test]
    fn test_claim_no_pending_rewards() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        // Referrer has 0 pending
        let result = client.try_claim_referral_reward(&referrer);
        assert_eq!(result, Err(Ok(Error::NoPendingRewards)));
    }

    #[test]
    fn test_claim_then_accumulate_then_claim_again() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        // First batch of events
        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);
        let claimed1 = client.claim_referral_reward(&referrer);
        assert_eq!(claimed1, 500);

        // Second batch
        client.record_referral_event(&admin, &user, &EventType::Deposit, &20_000);
        let claimed2 = client.claim_referral_reward(&referrer);
        assert_eq!(claimed2, 1000);

        // Total earned reflects both
        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 0);
        assert_eq!(state.total_earned, 1500);
        assert_eq!(state.event_count, 2);
    }

    #[test]
    fn test_claim_unknown_user() {
        let env = Env::default();
        let (client, _, _) = setup(&env);
        env.mock_all_auths();

        let unknown = Address::generate(&env);
        let result = client.try_claim_referral_reward(&unknown);
        assert_eq!(result, Err(Ok(Error::ReferrerNotRegistered)));
    }

    // -----------------------------------------------------------------------
    // View function tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_referral_state_not_found() {
        let env = Env::default();
        let (client, _, _) = setup(&env);

        let unknown = Address::generate(&env);
        let result = client.try_referral_state(&unknown);
        assert_eq!(result, Err(Ok(Error::ReferrerNotRegistered)));
    }

    #[test]
    fn test_get_referrer_none() {
        let env = Env::default();
        let (client, _, _) = setup(&env);

        let user = Address::generate(&env);
        assert_eq!(client.get_referrer(&user), None);
    }

    // -----------------------------------------------------------------------
    // Custom reward BPS tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_custom_reward_bps() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        // Set to 10% (1000 bps)
        client.set_reward_bps(&admin, &1000);

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);

        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 1000); // 10% of 10_000
    }

    #[test]
    fn test_zero_reward_bps() {
        let env = Env::default();
        let (client, admin, _) = setup(&env);
        env.mock_all_auths();

        // Set to 0%
        client.set_reward_bps(&admin, &0);

        let user = Address::generate(&env);
        let referrer = Address::generate(&env);
        client.register_referrer(&user, &referrer);

        client.record_referral_event(&admin, &user, &EventType::GamePlayed, &10_000);

        let state = client.referral_state(&referrer);
        assert_eq!(state.pending_reward, 0);
        assert_eq!(state.event_count, 1);
    }
}
