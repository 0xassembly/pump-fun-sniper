use {
    serde::Deserialize,
    solana_sdk::pubkey::Pubkey,
};

#[derive(Debug, Deserialize)]
pub struct ValidatorApiResponse {
    pub validators: Vec<JitoValidator>,
}

#[derive(Debug, Deserialize)]
pub struct JitoValidator {
    pub vote_account: String,
    pub running_jito: bool,
}

#[derive(Debug, Clone)]
pub struct TipStreamInfo {
    pub landed_tips_75th_percentile: f64,
    pub landed_tips_95th_percentile: f64,
    pub landed_tips_99th_percentile: f64,
}

impl Default for TipStreamInfo {
    fn default() -> Self {
        Self {
            landed_tips_75th_percentile: 0.00001,
            landed_tips_95th_percentile: 0.00004,
            landed_tips_99th_percentile: 0.00008,
        }
    }
}

#[derive(Debug)]
pub struct ValidatorInfo {
    pub node_pubkey: Pubkey,
    pub vote_pubkey: Pubkey,
    pub is_jito: bool,
} 