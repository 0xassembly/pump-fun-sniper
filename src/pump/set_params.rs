use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo,
    instruction::AccountMeta,
    pubkey::Pubkey,
};

/// SetParams instruction data
#[derive(Debug, BorshSerialize, BorshDeserialize, Clone, PartialEq)]
pub struct SetParamsInstructionData {
    pub fee_recipient: Pubkey,
    pub initial_virtual_token_reserves: u64,
    pub initial_virtual_sol_reserves: u64,
    pub initial_real_token_reserves: u64,
    pub token_total_supply: u64,
    pub fee_basis_points: u64,
}

/// SetParams accounts struct
#[derive(Debug, Clone)]
pub struct SetParamsAccounts {
    pub global: AccountMeta,          // [WRITE] global
    pub user: AccountMeta,            // [WRITE, SIGNER] user
    pub system_program: AccountMeta,  // [] systemProgram
    pub event_authority: AccountMeta, // [] eventAuthority
    pub program: AccountMeta,         // [] program
}

impl SetParamsAccounts {
    pub fn new(
        global: Pubkey,
        user: Pubkey,
        system_program: Pubkey,
        event_authority: Pubkey,
        program: Pubkey,
    ) -> Self {
        Self {
            global: AccountMeta::new(global, true),  // true for writable
            user: AccountMeta::new(user, true),      // true for writable and signer
            system_program: AccountMeta::new_readonly(system_program, false),
            event_authority: AccountMeta::new_readonly(event_authority, false),
            program: AccountMeta::new_readonly(program, false),
        }
    }

    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            self.global.clone(),
            self.user.clone(),
            self.system_program.clone(),
            self.event_authority.clone(),
            self.program.clone(),
        ]
    }
}

// Note: We don't need these from Go version:
// 1. Builder pattern - Rust struct initialization is cleaner
// 2. Individual getters/setters - Rust public fields are better
// 3. Validation - Handled by Solana runtime and Borsh
// 4. Tree encoding - Not needed in Rust
// 5. Manual serialization - Handled by Borsh
// 6. BaseVariant - Using Rust enums instead
// 7. Encode/Decode - Handled by Borsh traits 