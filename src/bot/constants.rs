use solana_program::pubkey::Pubkey;



pub const COMPUTE_UNIT_LIMIT: u32 = 70_000; 
// Program ID
pub const  PROGRAM_ID: Pubkey = solana_program::pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const  GLOBAL_ID: Pubkey = solana_program::pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const  MAINNET_FEE_ID: Pubkey = solana_program::pubkey!("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM");
pub const  MINT_AUTH_ID: Pubkey = solana_program::pubkey!("TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM");
pub const  PUMP_METAPLEX_ID: Pubkey = solana_program::pubkey!("metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s");
pub const  DEVNET_FEE_ID: Pubkey = solana_program::pubkey!("68yFSZxzLWJXkxxRGydZ63C6mHx1NLEDWmwN9Lb5yySg");
pub const  EVENT_AUTH_ID: Pubkey = solana_program::pubkey!("Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1");
pub const  TOKEN_PROGRAM_ID: Pubkey = solana_program::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
// Instruction discriminators
pub const INSTRUCTION_INITIALIZE: [u8; 8] = [175, 175, 109, 31, 13, 152, 155, 237];
pub const INSTRUCTION_SET_PARAMS: [u8; 8] = [27, 234, 178, 52, 147, 2, 187, 141];
pub const INSTRUCTION_CREATE: [u8; 8] = [24, 30, 200, 40, 5, 28, 7, 119];
pub const INSTRUCTION_BUY: [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
pub const INSTRUCTION_SELL: [u8; 8] = [51, 230, 133, 164, 1, 127, 131, 173];
pub const INSTRUCTION_WITHDRAW: [u8; 8] = [183, 18, 70, 156, 148, 109, 161, 34];