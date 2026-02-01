//! Shared utilities and data structures for Stellarcade contracts.
#![no_std]

use soroban_sdk::{contracttype, Address};

/// Common error codes used across all contracts.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotAuthorized = 1,
    InsufficientBalance = 2,
    InvalidAmount = 3,
    Overflow = 4,
}

/// A standard configuration for platform-wide settings.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformConfig {
    pub admin: Address,
    pub fee_percentage: u32, // In basis points (e.g., 250 = 2.5%)
}

/// Constant for basis points divisor.
pub const BASIS_POINTS_DIVISOR: u32 = 10_000;

/// Helper to calculate fee based on amount and basis points.
pub fn calculate_fee(amount: i128, fee_bps: u32) -> i128 {
    (amount * fee_bps as i128) / BASIS_POINTS_DIVISOR as i128
}
