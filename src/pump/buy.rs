use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::AccountInfo,
    instruction::{AccountMeta, Instruction, CompiledInstruction},
    program_error::ProgramError,
    pubkey::Pubkey,
};
use solana_sdk::bs58;
use thiserror::Error;
use std::str::FromStr;
use log::info;
use crate::bot::constants::{TOKEN_PROGRAM_ID, PROGRAM_ID, EVENT_AUTH_ID, GLOBAL_ID};
#[derive(Debug, Error)]
pub enum BuyError {
    #[error("Invalid account meta")]
    InvalidAccountMeta,
}

/// Buy instruction accounts struct
/// Note: We don't need getters/setters like in Go because Rust's struct fields can be public
#[derive(Debug)]
pub struct BuyAccounts {
    pub global: AccountMeta,             // [] global
    pub fee_recipient: AccountMeta,      // [WRITE] feeRecipient
    pub mint: AccountMeta,               // [] mint
    pub bonding_curve: AccountMeta,      // [WRITE] bondingCurve
    pub associated_bonding_curve: AccountMeta, // [WRITE] associatedBondingCurve
    pub associated_user: AccountMeta,    // [WRITE] associatedUser
    pub user: AccountMeta,               // [WRITE, SIGNER] user
    pub system_program: AccountMeta,     // [] systemProgram
    pub token_program: AccountMeta,      // [] tokenProgram
    pub rent: AccountMeta,               // [] rent
    pub event_authority: AccountMeta,    // [] eventAuthority
    pub program: AccountMeta,            // [] program
}

/// Buy instruction data
#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct BuyInstructionData {
    pub amount: u64,
    pub max_sol_cost: u64,
}

impl BuyAccounts {
    pub fn new(
        global: Pubkey,
        fee_recipient: Pubkey,
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
            fee_recipient: AccountMeta::new(fee_recipient, false),
            mint: AccountMeta::new(mint, false),
            bonding_curve: AccountMeta::new(bonding_curve, false),
            associated_bonding_curve: AccountMeta::new(associated_bonding_curve, false),
            associated_user: AccountMeta::new(associated_user, false),
            user: AccountMeta::new(user, true),
            system_program: AccountMeta::new_readonly(system_program, false),
            token_program: AccountMeta::new_readonly(token_program, false),
            rent: AccountMeta::new_readonly(rent, false),
            event_authority: AccountMeta::new_readonly(event_authority, false),
            program: AccountMeta::new_readonly(program, false),
        }
    }

    pub fn validate(&self) -> Result<(), ProgramError> {
        Ok(())
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
            self.token_program.clone(),
            self.rent.clone(),
            self.event_authority.clone(),
            self.program.clone(),
        ]
    }

    pub fn try_from_instruction(instruction: &CompiledInstruction, account_keys: &[Pubkey]) -> Result<Self, BuyError> {
        // First resolve all accounts like in Go's ResolveInstructionAccounts
        let resolved_accounts: Vec<AccountMeta> = instruction.accounts.iter()
            .map(|account_idx| {
                let pubkey = account_keys[*account_idx as usize];
                // In Go, Meta() creates a read-only non-signer by default
                AccountMeta::new_readonly(pubkey, false)
            })
            .collect();

        if resolved_accounts.len() != 12 {
            return Err(BuyError::InvalidAccountMeta);
        }

        // Then set the correct permissions as per the Go version
        Ok(Self {
            global: resolved_accounts[0].clone(),                                  // [] global
            fee_recipient: AccountMeta::new(resolved_accounts[1].pubkey, true),   // [WRITE] feeRecipient
            mint: resolved_accounts[2].clone(),                                   // [] mint
            bonding_curve: AccountMeta::new(resolved_accounts[3].pubkey, true),   // [WRITE] bondingCurve
            associated_bonding_curve: AccountMeta::new(resolved_accounts[4].pubkey, true), // [WRITE] associatedBondingCurve
            associated_user: AccountMeta::new(resolved_accounts[5].pubkey, true),  // [WRITE] associatedUser
            user: AccountMeta::new(resolved_accounts[6].pubkey, true),            // [WRITE, SIGNER] user
            system_program: resolved_accounts[7].clone(),                         // [] systemProgram
            token_program: resolved_accounts[8].clone(),                         // [] tokenProgram
            rent: resolved_accounts[9].clone(),                                  // [] rent
            event_authority: resolved_accounts[10].clone(),                      // [] eventAuthority
            program: resolved_accounts[11].clone(),                              // [] program
        })
    }


    pub fn decode_account_keys(account_keys: &Vec<Vec<u8>>) -> Result<Self, BuyError> {
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

        if resolved_accounts.len() <= 10 {
            return Err(BuyError::InvalidAccountMeta);
        }

        let mint = if resolved_accounts[8].pubkey == GLOBAL_ID {
            resolved_accounts[10].clone().pubkey
        } else {
            resolved_accounts[8].clone().pubkey
        };
        // Then set the correct permissions as per the Go version
        let accounts = Self {
            global: AccountMeta::new_readonly(GLOBAL_ID, false),                                  // [] global
            fee_recipient: AccountMeta::new(resolved_accounts[2].pubkey, true),   // [WRITE] feeRecipient
            mint: AccountMeta::new_readonly(mint , false),                                   // [] mint
            bonding_curve: AccountMeta::new(resolved_accounts[3].pubkey, true),   // [WRITE] bondingCurve
            associated_bonding_curve: AccountMeta::new(resolved_accounts[4].pubkey, true), // [WRITE] associatedBondingCurve
            associated_user: AccountMeta::new(resolved_accounts[1].pubkey, true),  // [WRITE] associatedUser
            user: AccountMeta::new(resolved_accounts[0].pubkey, true),            // [WRITE, SIGNER] user
            system_program: AccountMeta::new_readonly(resolved_accounts[9].clone().pubkey, false),                         // [] systemProgram
            token_program: AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),                         // [] tokenProgram
            rent: AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),                                  // [] rent
            event_authority: AccountMeta::new_readonly(EVENT_AUTH_ID, false),                      // [] eventAuthority
            program: AccountMeta::new_readonly(PROGRAM_ID, false),                              // [] program
        };
        Ok(accounts)
    }


}

// Note: We don't need most of the Go boilerplate because:
// 1. Solana's runtime handles account validation
// 2. Borsh handles serialization/deserialization
// 3. Rust's type system handles type safety
// 4. Account metadata is handled by Solana's AccountMeta