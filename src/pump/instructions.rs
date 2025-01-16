//! Pump Fun Instruction Handling Module
//! 
//! # Performance Considerations & Differences from Go Version
//! 
//! ## Why Not Using Anchor?
//! While Anchor provides convenient abstractions and safety features, we opt not to use it because:
//! 1. Performance Critical: This is a high-frequency trading bot where microseconds matter
//! 2. Anchor's runtime validation adds overhead that we can't afford in this use case
//! 3. Anchor's account validation checks add latency to instruction processing
//! 4. Manual instruction handling gives us more control over execution timing
//! 
//! ## Key Differences from Go Version:
//! 1. Go version uses `BaseVariant` and manual type registration:
//!    ```go
//!    type Instruction struct {
//!        ag_binary.BaseVariant
//!    }
//!    ```
//!    Rust version uses enum for better performance:
//!    ```rust
//    pub enum PumpInstruction {
//        Initialize(Initialize),
//!        // ...
//!    }
//!    ```
//! 
//! 2. Go version requires runtime type checking:
//!    ```go
//!    if v, ok := inst.Impl.(ag_solanago.AccountsSettable); ok {
//!        err := v.SetAccounts(accounts)
//!    }
//!    ```
//!    Rust version uses compile-time type checking for better performance
//! 
//! 3. Go version uses reflection-based encoding:
//!    ```go
//!    ag_binary.NewBorshEncoder(buf).Encode(inst)
//!    ```
//!    Rust version uses direct byte manipulation:
//!    ```rust
//!    buf.extend_from_slice(&INSTRUCTION_INITIALIZE)
//!    ```
//! 
//! ## Performance Benefits:
//! 1. No runtime type checking overhead
//! 2. No reflection-based serialization
//! 3. No dynamic dispatch
//! 4. Direct memory access
//! 5. Compile-time optimizations
//! 6. Minimal instruction processing latency
//! 
//! These optimizations are crucial for a trading bot where execution speed
//! directly impacts the ability to capture market opportunities.

use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    program_error::ProgramError,
};
use crate::pump::{buy::BuyInstructionData, 
    sell::SellInstructionData,  
    create::CreateInstructionData
};
use std::mem::size_of;
use std::collections::HashMap;
use lazy_static::lazy_static;
use crate::bot::constants::{INSTRUCTION_BUY, INSTRUCTION_CREATE, INSTRUCTION_SELL};


#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Initialize {
    pub fee_micro_lamports: u64,
}


#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Withdraw {
    pub amount: u64,
}



#[derive(Debug)]
pub enum PumpInstruction {
    Buy {
        data: BuyInstructionData,
        accounts: Vec<AccountMeta>,
    },
    Create {
        data: CreateInstructionData,
        accounts: Vec<AccountMeta>,
    },
    SellInstructionData(SellInstructionData),
}

// Add this struct for instruction identification
#[derive(Clone)]
pub struct InstructionID {
    pub name: String,
}

// Add PUMP_IDS mapping using lazy_static
lazy_static! {
    pub static ref PUMP_IDS: HashMap<[u8; 8], InstructionID> = {
        let mut m = HashMap::new();
        m.insert(
            INSTRUCTION_CREATE,
            InstructionID {
                name: "create".to_string(),
            }
        );
        m.insert(
            INSTRUCTION_BUY,
            InstructionID {
                name: "buy".to_string(),
            }
        );
        m.insert(
            INSTRUCTION_SELL,
            InstructionID {
                name: "sell".to_string(),
            }
        );
        m
    };
}

impl PumpInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        if input.len() < 8 {
            return Err(ProgramError::InvalidInstructionData);
        }

        let (tag, rest) = input.split_at(8);
        match tag.try_into().unwrap() {
            INSTRUCTION_BUY => {
                let data = BuyInstructionData::try_from_slice(rest)?;
                Ok(PumpInstruction::Buy { 
                    data,
                    accounts: Vec::new(),
                })
            }
            INSTRUCTION_CREATE => {
                let data = CreateInstructionData::try_from_slice(rest)?;
                Ok(PumpInstruction::Create {
                    data,
                    accounts: Vec::new(),
                })
            }
            INSTRUCTION_SELL => {
                let payload = SellInstructionData::try_from_slice(rest)?;
                Ok(PumpInstruction::SellInstructionData(payload))
            }

            _ => Err(ProgramError::InvalidInstructionData),
        }
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(size_of::<Self>());
        match self {
            PumpInstruction::Buy { data, accounts } => {
                buf.extend_from_slice(&INSTRUCTION_BUY);
                buf.extend_from_slice(&data.try_to_vec().unwrap());
            }
            PumpInstruction::Create { data, accounts } => {
                buf.extend_from_slice(&INSTRUCTION_CREATE);
                buf.extend_from_slice(&data.try_to_vec().unwrap());
            }

            PumpInstruction::SellInstructionData(payload) => {
                buf.extend_from_slice(&INSTRUCTION_SELL);
                buf.extend_from_slice(&payload.try_to_vec().unwrap());
            }

        }
        buf
    }

    pub fn data(&self) -> Result<Vec<u8>, ProgramError> {
        Ok(self.pack())
    }
}

pub fn decode_instruction(accounts: &[AccountMeta], data: &[u8]) -> Result<PumpInstruction, ProgramError> {
    let mut instruction = PumpInstruction::unpack(data)?;
    
    // Set accounts based on instruction type
    match &mut instruction {
        PumpInstruction::Buy { accounts: inst_accounts, .. } => {
            if accounts.len() != 12 {
                return Err(ProgramError::InvalidAccountData);
            }
            *inst_accounts = accounts.to_vec();
        }
        PumpInstruction::Create { accounts: inst_accounts, .. } => {
            if accounts.len() != 14 {
                return Err(ProgramError::InvalidAccountData);
            }
            *inst_accounts = accounts.to_vec();
        }
        PumpInstruction::SellInstructionData(_) => {
            // Sell doesn't store accounts
        }
    }
    
    Ok(instruction)
}
