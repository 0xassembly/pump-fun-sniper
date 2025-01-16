use {
    solana_client::{
        rpc_client::RpcClient,
        nonblocking::rpc_client::RpcClient as NonblockingRpcClient,
    },
    solana_program::{pubkey::Pubkey},
    solana_sdk::commitment_config::CommitmentConfig,
    std::{fmt, error::Error},
    std::sync::Arc,
};

/// BondingCurveData holds the relevant information decoded from the on-chain data
#[derive(Debug, Clone)]
pub struct BondingCurveData {
    pub real_token_reserves: u128,
    pub virtual_token_reserves: u128,
    pub virtual_sol_reserves: u128,
}
impl Default for BondingCurveData {
    fn default() -> Self {
        Self {
            virtual_sol_reserves: 0,
            virtual_token_reserves: 0,
            real_token_reserves: 0,
        }
    }
}
impl fmt::Display for BondingCurveData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RealTokenReserves={}, VirtualTokenReserves={}, VirtualSolReserves={}",
            self.real_token_reserves, self.virtual_token_reserves, self.virtual_sol_reserves
        )
    }
}

impl BondingCurveData {
    /// Fetches the bonding curve data from the blockchain and decodes it
    pub async fn fetchBondingCurve(
        rpc_client: &Arc<NonblockingRpcClient>,
        bonding_curve_pubkey: &Pubkey,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let account = rpc_client.get_account_with_commitment(
            bonding_curve_pubkey,
            CommitmentConfig::processed(),
        ).await?.value.ok_or("Account not found")?;

        let data = account.data;
        if data.len() < 24 {
            return Err(Box::<dyn std::error::Error + Send + Sync>::from("FBCD: insufficient data length"));
        }

        // Decode the bonding curve data using little-endian format
        let real_token_reserves = u64::from_le_bytes(data[0..8].try_into()?) as u128;
        let virtual_token_reserves = u64::from_le_bytes(data[8..16].try_into()?) as u128;
        let virtual_sol_reserves = u64::from_le_bytes(data[16..24].try_into()?) as u128;

        Ok(Self {
            real_token_reserves,
            virtual_token_reserves,
            virtual_sol_reserves,
        })
    }

    /// Calculates how many tokens can be purchased given a specific amount of SOL
    pub fn calculate_buy_quote(&self, sol_amount: u64, percentage: f64) -> u128 {
        // Convert to u128 for intermediate calculations to avoid overflow
        let sol_amount = sol_amount as u128;
        let virtual_sol_reserves = self.virtual_sol_reserves;
        let virtual_token_reserves = self.virtual_token_reserves;

        // Compute the new virtual reserves
        let new_virtual_sol_reserves = virtual_sol_reserves.saturating_add(sol_amount);
        
        // Calculate invariant and new virtual token reserves
        let invariant = virtual_sol_reserves.saturating_mul(virtual_token_reserves);
        let new_virtual_token_reserves = invariant.saturating_div(new_virtual_sol_reserves);

        // Calculate the tokens to buy
        let tokens_to_buy = virtual_token_reserves.saturating_sub(new_virtual_token_reserves);

        // Apply the percentage reduction (e.g., 95% or 0.95)
        let final_tokens = (tokens_to_buy as f64 * percentage) as u128;

        final_tokens
    }


    pub fn calculate_sell_quote(&self, token_amount: u64, percentage: f64) -> u128 {
        let token_amount = token_amount as u128;
        let virtual_sol_reserves = self.virtual_sol_reserves;
        let virtual_token_reserves = self.virtual_token_reserves;

        // Calculate new virtual token reserves
        let new_virtual_token_reserves = virtual_token_reserves.saturating_sub(token_amount);
        
        // Calculate invariant and new virtual sol reserves
        let invariant = virtual_sol_reserves.saturating_mul(virtual_token_reserves);
        let new_virtual_sol_reserves = invariant.saturating_div(new_virtual_token_reserves);

        // Calculate the SOL amount we'll receive
        let sol_amount = new_virtual_sol_reserves.saturating_sub(virtual_sol_reserves);

        // Apply the percentage reduction (e.g., 98% or 0.98)
        let final_sol = (sol_amount as f64 * percentage) as u128;

        final_sol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_buy_quote() {
        let curve = BondingCurveData {
            real_token_reserves: 1000,
            virtual_token_reserves: 10000,
            virtual_sol_reserves: 5000,
        };

        // Test with 95% percentage
        let quote = curve.calculate_buy_quote(100, 0.95);
        assert!(quote > 0, "Buy quote should be greater than 0");

        // Test with 100% percentage
        let quote_full = curve.calculate_buy_quote(100, 1.0);
        assert!(quote_full > quote, "100% quote should be greater than 95% quote");

        // Test with large numbers
        let large_curve = BondingCurveData {
            real_token_reserves: u128::MAX / 4,
            virtual_token_reserves: u128::MAX / 4,
            virtual_sol_reserves: u128::MAX / 4,
        };
        let large_quote = large_curve.calculate_buy_quote(u64::MAX, 1.0);
        assert!(large_quote > 0, "Large buy quote should not overflow");
    }
}
