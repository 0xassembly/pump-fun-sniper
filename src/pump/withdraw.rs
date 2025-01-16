use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo,
    instruction::AccountMeta,
    pubkey::Pubkey,
};

/// Withdraw accounts struct
/// Allows the admin to withdraw liquidity for a migration once the bonding curve completes
#[derive(Debug, Clone)]
pub struct WithdrawAccounts {
    pub global: AccountMeta,                    // [] global
    pub mint: AccountMeta,                      // [] mint
    pub bonding_curve: AccountMeta,             // [WRITE] bondingCurve
    pub associated_bonding_curve: AccountMeta,  // [WRITE] associatedBondingCurve
    pub associated_user: AccountMeta,           // [WRITE] associatedUser
    pub user: AccountMeta,                      // [WRITE, SIGNER] user
    pub system_program: AccountMeta,            // [] systemProgram
    pub token_program: AccountMeta,             // [] tokenProgram
    pub rent: AccountMeta,                      // [] rent
    pub event_authority: AccountMeta,           // [] eventAuthority
    pub program: AccountMeta,                   // [] program
}

// Note: No instruction data needed as the Go version doesn't have any parameters

impl WithdrawAccounts {
    pub fn new(
        global: Pubkey,
        mint: Pubkey,
        bonding_curve: Pubkey,
        associated_bonding_curve: Pubkey,
        associated_user: Pubkey,
        user: Pubkey,
        system_program: Pubkey,
        token_program: Pubkey,
        rent: Pubkey,
        event_authority: Pubkey,
        program: Pubkey,
    ) -> Self {
        Self {
            global: AccountMeta::new_readonly(global, false),
            mint: AccountMeta::new_readonly(mint, false),
            bonding_curve: AccountMeta::new(bonding_curve, true),           // true for writable
            associated_bonding_curve: AccountMeta::new(associated_bonding_curve, true), // true for writable
            associated_user: AccountMeta::new(associated_user, true),       // true for writable
            user: AccountMeta::new(user, true),                            // true for writable and signer
            system_program: AccountMeta::new_readonly(system_program, false),
            token_program: AccountMeta::new_readonly(token_program, false),
            rent: AccountMeta::new_readonly(rent, false),
            event_authority: AccountMeta::new_readonly(event_authority, false),
            program: AccountMeta::new_readonly(program, false),
        }
    }

    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            self.global.clone(),
            self.mint.clone(),
            self.bonding_curve.clone(),
            self.associated_bonding_curve.clone(),
            self.associated_user.clone(),
            self.user.clone(),
            self.system_program.clone(),
            self.token_program.clone(),
            self.rent.clone(),
            self.event_authority.clone(),
            self.program.clone(),
        ]
    }
}

// Note: We don't need these from Go version:
// 1. Builder pattern - Rust struct initialization is cleaner
// 2. Individual getters/setters - Rust public fields are better
// 3. Validation - Handled by Solana runtime
// 4. Tree encoding - Not needed in Rust
// 5. Manual serialization - Not needed as there's no instruction data
// 6. BaseVariant - Using Rust enums instead
// 7. Encode/Decode - Not needed as there's no instruction data 