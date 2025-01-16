use {
    borsh::{BorshDeserialize, BorshSerialize},
    solana_program::{
        instruction::{AccountMeta, Instruction, CompiledInstruction},
        program_error::ProgramError,
        pubkey::Pubkey,
        system_program,
        sysvar::rent,
    },
    spl_token,
    spl_associated_token_account,
    thiserror::Error,
};

pub const INSTRUCTION_TYPE_CREATE: u8 = 0;

#[derive(Debug, Error)]
pub enum CreateError {
    #[error("Name parameter is not set")]
    NameNotSet,
    #[error("Symbol parameter is not set")]
    SymbolNotSet,
    #[error("Uri parameter is not set")]
    UriNotSet,
    #[error("Invalid account meta")]
    InvalidAccountMeta,
}

/// Creates a new coin and bonding curve.
#[derive(Debug, BorshSerialize, BorshDeserialize)]
pub struct CreateInstructionData {
    /// Name of the coin
    pub name: String,
    /// Symbol of the coin
    pub symbol: String,
    /// Uri for metadata
    pub uri: String,
}

/// Accounts required for the Create instruction
#[derive(Debug)]
pub struct CreateAccounts {
    pub mint: AccountMeta,                      // [WRITE, SIGNER] mint
    pub mint_authority: AccountMeta,            // [] mintAuthority
    pub bonding_curve: AccountMeta,             // [WRITE] bondingCurve
    pub associated_bonding_curve: AccountMeta,  // [WRITE] associatedBondingCurve
    pub global: AccountMeta,                    // [] global
    pub mpl_token_metadata: AccountMeta,        // [] mplTokenMetadata
    pub metadata: AccountMeta,                  // [WRITE] metadata
    pub user: AccountMeta,                      // [WRITE, SIGNER] user
    pub system_program: AccountMeta,            // [] systemProgram
    pub token_program: AccountMeta,             // [] tokenProgram
    pub associated_token_program: AccountMeta,  // [] associatedTokenProgram
    pub rent: AccountMeta,                      // [] rent
    pub event_authority: AccountMeta,           // [] eventAuthority
    pub program: AccountMeta,                   // [] program
}

impl CreateAccounts {
    pub fn new(
        mint: Pubkey,
        mint_authority: Pubkey,
        bonding_curve: Pubkey,
        associated_bonding_curve: Pubkey,
        global: Pubkey,
        mpl_token_metadata: Pubkey,
        metadata: Pubkey,
        user: Pubkey,
        system_program: Pubkey,
        token_program: Pubkey,
        associated_token_program: Pubkey,
        rent: Pubkey,
        event_authority: Pubkey,
        program: Pubkey,
    ) -> Self {
        Self {
            mint: AccountMeta::new(mint, true),
            mint_authority: AccountMeta::new_readonly(mint_authority, false),
            bonding_curve: AccountMeta::new(bonding_curve, true),
            associated_bonding_curve: AccountMeta::new(associated_bonding_curve, true),
            global: AccountMeta::new_readonly(global, false),
            mpl_token_metadata: AccountMeta::new_readonly(mpl_token_metadata, false),
            metadata: AccountMeta::new(metadata, true),
            user: AccountMeta::new(user, true),
            system_program: AccountMeta::new_readonly(system_program, false),
            token_program: AccountMeta::new_readonly(token_program, false),
            associated_token_program: AccountMeta::new_readonly(associated_token_program, false),
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
            self.mint.clone(),
            self.mint_authority.clone(),
            self.bonding_curve.clone(),
            self.associated_bonding_curve.clone(),
            self.global.clone(),
            self.mpl_token_metadata.clone(),
            self.metadata.clone(),
            self.user.clone(),
            self.system_program.clone(),
            self.token_program.clone(),
            self.associated_token_program.clone(),
            self.rent.clone(),
            self.event_authority.clone(),
            self.program.clone(),
        ]
    }
}

pub struct CreateInstructionWithAccounts<'a> {
    pub instruction: &'a CompiledInstruction,
    pub account_keys: &'a [Pubkey],
}

impl<'a> TryFrom<CreateInstructionWithAccounts<'a>> for CreateAccounts {
    type Error = CreateError;

    fn try_from(value: CreateInstructionWithAccounts) -> Result<Self, Self::Error> {
        if value.instruction.accounts.len() != 14 {
            return Err(CreateError::InvalidAccountMeta);
        }

        Ok(Self {
            mint: AccountMeta::new(value.account_keys[value.instruction.accounts[0] as usize], true),
            mint_authority: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[1] as usize], false),
            bonding_curve: AccountMeta::new(value.account_keys[value.instruction.accounts[2] as usize], true),
            associated_bonding_curve: AccountMeta::new(value.account_keys[value.instruction.accounts[3] as usize], true),
            global: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[4] as usize], false),
            mpl_token_metadata: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[5] as usize], false),
            metadata: AccountMeta::new(value.account_keys[value.instruction.accounts[6] as usize], true),
            user: AccountMeta::new(value.account_keys[value.instruction.accounts[7] as usize], true),
            system_program: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[8] as usize], false),
            token_program: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[9] as usize], false),
            associated_token_program: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[10] as usize], false),
            rent: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[11] as usize], false),
            event_authority: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[12] as usize], false),
            program: AccountMeta::new_readonly(value.account_keys[value.instruction.accounts[13] as usize], false),
        })
    }
}

impl std::fmt::Display for CreateInstructionData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Create Instruction:")?;
        writeln!(f, "  Name: {}", self.name)?;
        writeln!(f, "Symbol: {}", self.symbol)?;
        writeln!(f, "   Uri: {}", self.uri)?;
        Ok(())
    }
}