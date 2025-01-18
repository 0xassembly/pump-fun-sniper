use {
    crate::{
        bot::{Bot, structs::Coin},
        pump::sell::{SellAccounts, SellInstructionData},
        bot::constants::{COMPUTE_UNIT_LIMIT, INSTRUCTION_SELL},
    },
    solana_program::{
        instruction::Instruction,
        pubkey::Pubkey,
        system_program,
    },
    solana_sdk::{
        signature::{Keypair, Signature, Signer},
        transaction::Transaction,
        compute_budget::{ComputeBudgetInstruction},
        commitment_config::CommitmentConfig,
    },
    std::{
        sync::Arc,
        time::Duration,
    },
    tokio::{
        sync::{mpsc, Mutex},
        time::sleep,
    },
    log::{info, error},
    borsh::BorshSerialize,
    solana_client::rpc_config::RpcSendTransactionConfig,
    tokio::time::timeout,
};

impl Bot {
    /// Sells a coin quickly by sending multiple transactions
    /// Similar to Go's SellCoinFast, but using Rust's async/await
    pub async fn sell_coin_fast(&self, coin: Arc<Mutex<Coin>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        {
            let mut coin_guard = coin.lock().await;
            info!("Preparing to sell coin {}", coin_guard.mint_address.unwrap());
            coin_guard.is_selling_coin = true;
        }

        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 10;
        const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(8); // 8 seconds timeout
        let mut send_vanilla = true;
        sleep(Duration::from_secs(4)).await; // Wait for 4 seconds before starting
        while retry_count < MAX_RETRIES {
            send_vanilla = !send_vanilla;
            
            // Attempt to sell with timeout
            match timeout(TRANSACTION_TIMEOUT, self.sell_coin(&coin, send_vanilla)).await {
                Ok(result) => {
                    match result {
                        Ok(_) => {
                            info!("Sell transaction confirmed");
                            let mut coin_guard = coin.lock().await;
                            coin_guard.exited_sell_coin = true;
                            return Ok(());
                        }
                        Err(e) => {
                            retry_count += 1;
                            info!("Error sending sell transaction (attempt {}/{}): {}", retry_count, MAX_RETRIES, e);
                        }
                    }
                }
                Err(_) => {
                    retry_count += 1;
                    info!("Sell transaction timed out after {} seconds (attempt {}/{})", 
                         TRANSACTION_TIMEOUT.as_secs(), retry_count, MAX_RETRIES);
                }
            }

            if retry_count >= MAX_RETRIES {
                error!("Max retries reached for sell transaction");
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        Err("Failed to sell after max retries".into())
    }

    /// Sells a coin with the given parameters
    pub async fn sell_coin(&self, coin: &Arc<Mutex<Coin>>, send_vanilla: bool) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let coin_guard = coin.lock().await;
        
        info!("Starting sell transaction for coin: {}", coin_guard.mint_address.unwrap());
        
        // Create sell instruction
        let sell_instruction = self.create_sell_instruction(&coin_guard)
            .map_err(|e| format!("Failed to create sell instruction: {}", e))?;
        
        info!("Fee micro lamports: {}", self.fee_micro_lamports);
        // Create compute budget instructions
        let mut instructions = 
        vec![
            ComputeBudgetInstruction::set_compute_unit_price(
                self.fee_micro_lamports
            ),
            ComputeBudgetInstruction::set_compute_unit_limit(
                COMPUTE_UNIT_LIMIT
            ),
            sell_instruction,
        ];

        // Handle Jito if enabled
        //let enable_jito = self.jito_manager.is_jito_leader() && !send_vanilla;
        let enable_jito = false;
        info!("Enable Jito Sell: {}", enable_jito);
        if enable_jito {
            coin_guard.status("Jito leader, setting default tip & removing priority fee inst");
            let tip_inst = self.jito_manager.generate_default_tip_instruction().await
                .map_err(|e| format!("Failed to generate Jito default tip instruction: {}", e))?;
            instructions.push(tip_inst);
            // Remove priority fee when using Jito tip
            instructions.remove(0);
        }
        
        // Create and send transaction
        let transaction = self.create_transaction(instructions).await
            .map_err(|e| format!("Failed to create transaction: {}", e))?;
        
        info!("Created transaction successfully, attempting to send...");
        
        let signature = if enable_jito {
            info!("Sending transaction via Jito");
            self.jito_manager.send_transaction(&transaction).await
            .map_err(|e| format!("Failed to send transaction via Jito: {}", e))?
        } else {
            let config = RpcSendTransactionConfig {
                skip_preflight: true,
                .. RpcSendTransactionConfig::default()
            };

            self.rpc_client.send_and_confirm_transaction_with_spinner_and_config(
                &transaction,
                CommitmentConfig::finalized(),
                config,
            ).await
                .map_err(|e| format!("Failed to send transaction: {}", e))?
                .to_string()
        };

        Ok(signature)
    }

    /// Creates a sell instruction
    fn create_sell_instruction(&self, coin: &Coin) -> Result<Instruction, Box<dyn std::error::Error + Send + Sync>> {
        // Validate required addresses
        let mint_address = coin.mint_address.ok_or_else(|| Box::<dyn std::error::Error + Send + Sync>::from("Mint address is None"))?;
        let associated_token_account = coin.associated_token_account.ok_or_else(|| Box::<dyn std::error::Error + Send + Sync>::from("Associated token account is None"))?;
        info!("Creating sell instruction for:");
        info!("Mint: {}", mint_address);
        info!("Token Bonding Curve: {}", coin.token_bonding_curve);
        info!("Associated Bonding Curve: {}", coin.associated_bonding_curve);
        info!("Event Authority: {}", coin.event_authority);
        info!("Tokens to sell: {}", coin.tokens_held);

        // Minimum of 1 lamport to ensure we get filled at any price
        let minimum_lamports = 1;

        let accounts = SellAccounts::new(
            self.global_address,
            self.fee_id,
            mint_address,
            coin.token_bonding_curve,
            coin.associated_bonding_curve,
            associated_token_account,
            self.private_key.pubkey(),
            system_program::id(),
            spl_associated_token_account::id(),
            spl_token::id(),
            coin.event_authority,
            self.program_id,
        );

        let data = SellInstructionData {
            amount: coin.tokens_held,
            min_sol_output: minimum_lamports,
        };

        // Add sell discriminator
        let mut instruction_data = INSTRUCTION_SELL.to_vec();
        instruction_data.extend_from_slice(&data.try_to_vec().unwrap());

        Ok(Instruction {
            program_id: self.program_id,
            accounts: accounts.to_account_metas(),
            data: instruction_data,
        })
    }
}
