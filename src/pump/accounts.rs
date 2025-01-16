use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

// Global account discriminator
pub const GLOBAL_DISCRIMINATOR: [u8; 8] = [167, 232, 232, 177, 200, 108, 114, 127];

// BondingCurve account discriminator
pub const BONDING_CURVE_DISCRIMINATOR: [u8; 8] = [23, 183, 248, 55, 96, 216, 172, 96];

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct Global {
    pub initialized: bool,
    pub authority: Pubkey,
    pub fee_recipient: Pubkey,
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
}

impl Global {
    pub const LEN: usize = 8 + // discriminator
        1 + // initialized
        32 + // authority
        32 + // fee_recipient
        8 + // initial_virtual_token_reserves
        8 + // initial_virtual_sol_reserves
        8 + // initial_real_token_reserves
        8 + // token_total_supply
        8; // fee_basis_points
}

#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct BondingCurve {
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
}

impl BondingCurve {
    pub const LEN: usize = 8 + // discriminator
        8 + // virtual_token_reserves
        8 + // virtual_sol_reserves
        8 + // real_token_reserves
        8 + // real_sol_reserves
        8 + // token_total_supply
        1; // complete
}
