use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo,
    instruction::{AccountMeta, CompiledInstruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};
use solana_sdk::bs58;
use thiserror::Error;
use std::str::FromStr;
use log::info;

use crate::bot::constants::TOKEN_PROGRAM_ID;
#[derive(Debug)]
pub enum SellError {
    InvalidAccountMeta,
}

/// Sell instruction accounts struct
/// Note: We don't need getters/setters like in Go because Rust's struct fields can be public
#[derive(Debug)]
pub struct SellAccounts {
    pub global: AccountMeta,             // [] global
    pub fee_recipient: AccountMeta,      // [WRITE] feeRecipient
    pub mint: AccountMeta,               // [] mint
    pub bonding_curve: AccountMeta,      // [WRITE] bondingCurve
    pub associated_bonding_curve: AccountMeta, // [WRITE] associatedBondingCurve
    pub associated_user: AccountMeta,    // [WRITE] associatedUser
    pub user: AccountMeta,               // [WRITE, SIGNER] user
    pub system_program: AccountMeta,     // [] systemProgram
    pub associated_token_program: AccountMeta, // [] associatedTokenProgram
    pub token_program: AccountMeta,      // [] tokenProgram
    pub event_authority: AccountMeta,    // [] eventAuthority
    pub program: AccountMeta,            // [] program
}

/// Sell instruction data
#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct SellInstructionData {
    pub amount: u64,
    pub min_sol_output: u64,
}

impl SellAccounts {
    pub fn new(
        global: Pubkey,
        fee_recipient: Pubkey,
        mint: Pubkey,
        bonding_curve: Pubkey,
        associated_bonding_curve: Pubkey,
        associated_user: Pubkey,
        user: Pubkey,
        system_program: Pubkey,
        associated_token_program: Pubkey,
        token_program: Pubkey,
        event_authority: Pubkey,
        program: Pubkey,
    ) -> Self {
        Self {
            global: AccountMeta::new_readonly(global, false),
            fee_recipient: AccountMeta::new(fee_recipient, false),
            mint: AccountMeta::new_readonly(mint, false),
            bonding_curve: AccountMeta::new(bonding_curve, false),
            associated_bonding_curve: AccountMeta::new(associated_bonding_curve, false),
            associated_user: AccountMeta::new(associated_user, false),
            user: AccountMeta::new(user, true), // true = signer
            system_program: AccountMeta::new_readonly(system_program, false),
            associated_token_program: AccountMeta::new_readonly(associated_token_program, false),
            token_program: AccountMeta::new_readonly(token_program, false),
            event_authority: AccountMeta::new_readonly(event_authority, false),
            program: AccountMeta::new_readonly(program, false),
        }
    }

    pub fn to_account_metas(&self) -> Vec<AccountMeta> {
        vec![
            self.global.clone(),
            self.fee_recipient.clone(),
            self.mint.clone(),
            self.bonding_curve.clone(),
            self.associated_bonding_curve.clone(),
            self.associated_user.clone(),
            self.user.clone(),
            self.system_program.clone(),
            self.associated_token_program.clone(),
            self.token_program.clone(),
            self.event_authority.clone(),
            self.program.clone(),
        ]
    }

    pub fn try_from_instruction(instruction: &CompiledInstruction, account_keys: &[Pubkey]) -> Result<Self, SellError> {
        // First resolve all accounts like in Go's ResolveInstructionAccounts
        let resolved_accounts: Vec<AccountMeta> = instruction.accounts.iter()
            .map(|account_idx| {
                let pubkey = account_keys[*account_idx as usize];
                // In Go, Meta() creates a read-only non-signer by default
                AccountMeta::new_readonly(pubkey, false)
            })
            .collect();

        if resolved_accounts.len() != 12 {
            return Err(SellError::InvalidAccountMeta);
        }

        // Then set the correct permissions as per the Go version
        Ok(Self {
            global: resolved_accounts[0].clone(),                                  // [] global
            fee_recipient: AccountMeta::new(resolved_accounts[1].pubkey, false),  // [WRITE] feeRecipient
            mint: resolved_accounts[2].clone(),                                   // [] mint
            bonding_curve: AccountMeta::new(resolved_accounts[3].pubkey, false),  // [WRITE] bondingCurve
            associated_bonding_curve: AccountMeta::new(resolved_accounts[4].pubkey, false), // [WRITE] associatedBondingCurve
            associated_user: AccountMeta::new(resolved_accounts[5].pubkey, false), // [WRITE] associatedUser
            user: AccountMeta::new(resolved_accounts[6].pubkey, true),            // [WRITE, SIGNER] user
            system_program: resolved_accounts[7].clone(),                         // [] systemProgram
            associated_token_program: resolved_accounts[8].clone(),               // [] associatedTokenProgram
            token_program: resolved_accounts[9].clone(),                         // [] tokenProgram
            event_authority: resolved_accounts[10].clone(),                      // [] eventAuthority
            program: resolved_accounts[11].clone(),                              // [] program
        })
    }

    
    pub fn decode_account_keys(account_keys: &Vec<Vec<u8>>) -> Result<Self, SellError> {
        // First resolve all accounts like in Go's ResolveInstructionAccounts
        // Print all account keys and store them in a vector
        info!("All account keys in message:");
        let resolved_accounts: Vec<AccountMeta> = account_keys.iter().enumerate()
            .map(|(i, key)| {
                let pubkey = Pubkey::from_str(&bs58::encode(key).into_string()).unwrap();
                info!("Account {}: {}", i, pubkey);
                AccountMeta::new_readonly(pubkey, false)
            })
            .collect();

        if resolved_accounts.len() <= 12 {
            return Err(SellError::InvalidAccountMeta);
        }

        // Then set the correct permissions as per the Go version
        let accounts = Self {
            global: AccountMeta::new_readonly(resolved_accounts[8].clone().pubkey, false),                                  // [] global
            fee_recipient: AccountMeta::new(resolved_accounts[1].pubkey, true),   // [WRITE] feeRecipient
            mint: AccountMeta::new_readonly(resolved_accounts[10].clone().pubkey, false),                                   // [] mint
            bonding_curve: AccountMeta::new(resolved_accounts[2].pubkey, true),   // [WRITE] bondingCurve
            associated_bonding_curve: AccountMeta::new(resolved_accounts[3].pubkey, true), // [WRITE] associatedBondingCurve
            associated_user: AccountMeta::new(resolved_accounts[4].pubkey, true),  // [WRITE] associatedUser
            user: AccountMeta::new(resolved_accounts[0].pubkey, true),            // [WRITE, SIGNER] user
            system_program: AccountMeta::new_readonly(resolved_accounts[12].clone().pubkey, false), 
            associated_token_program: AccountMeta::new_readonly(resolved_accounts[10].clone().pubkey, false),
            token_program: AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),                         // [] tokenProgram                         // [] rent
            event_authority: AccountMeta::new_readonly(resolved_accounts[9].clone().pubkey, false),                      // [] eventAuthority
            program: AccountMeta::new_readonly(resolved_accounts[11].clone().pubkey, false),                              // [] program
        };
        Ok(accounts)
    }

}

// Note: We don't need these from Go version:
// 1. Builder pattern - Rust struct initialization is cleaner
// 2. Individual getters/setters - Rust public fields are better
// 3. Validation - Handled by Solana runtime
// 4. Tree encoding - Not needed in Rust
// 5. Manual serialization - Handled by Borsh
// 6. BaseVariant - Using Rust enums instead 