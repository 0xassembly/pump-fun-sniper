#[derive(Debug, thiserror::Error)]
pub enum MonitorError {
    #[error("Bad create instruction")]
    BadCreateInstruction,
    
    #[error("No creator ATA")]
    NoCreatorATA,
    
    #[error("Error creating new coin")]
    CreatingNewCoin,
    
    #[error("No creator buy found")]
    NoCreatorBuy,

    #[error("No creator sell found")]
    NoCreatorSell,

    #[error("Buy amount can't be ZERO")]
    ZeroBuyAmount,
    
    #[error("Buy data can't be decoded")]
    BuyDataCantDecode,

    #[error("Buy accounts can't be decoded")]
    BuyAccountsCantDecode,

    #[error("Instruction data too short")]
    InstructionDataTooShort,

    #[error("No create instruction found")]
    NoCreateInstruction,

    #[error("No buy instruction found")]
    NoBuyInstruction,
}
