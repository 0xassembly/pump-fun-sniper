use {
    crate::{
        bot::{structs::Coin, utils::is_exchange_address, Bot, constants::{{INSTRUCTION_BUY, INSTRUCTION_CREATE}, PROGRAM_ID as PUMP_PROGRAM_ID, PUMP_METAPLEX_ID, MINT_AUTH_ID}},
        pump::{
            buy::{BuyAccounts,BuyInstructionData}, create::{CreateAccounts, CreateInstructionData, CreateInstructionWithAccounts},
        },
    }, bincode, borsh::BorshDeserialize, chrono::{DateTime, Utc}, futures::{SinkExt, Stream, StreamExt}, log::{debug, error, info}, solana_client::{
        nonblocking::{
            pubsub_client::{PubsubClient, PubsubClientError}, rpc_client::RpcClient
        },
        pubsub_client::PubsubClientSubscription,
        rpc_config::{RpcTransactionConfig, RpcTransactionLogsConfig, RpcTransactionLogsFilter},
    }, solana_program::{
        instruction::Instruction,
        program_pack::Pack,
        pubkey::Pubkey,
        system_instruction,
        system_program,
    }, solana_sdk::{
        bs58, commitment_config::CommitmentConfig, signature::Signature, transaction::Transaction
    }, solana_transaction_status::UiTransactionEncoding, std::{collections::HashMap, error::Error, str::FromStr, sync::Arc, time::{Duration, Instant}}, tokio::{sync::{mpsc::{self, UnboundedReceiver}, Mutex}, time::Instant as TokioInstant}, tonic::{
        codec::{CompressionEncoding, Streaming}, metadata::{errors::InvalidMetadataValue, AsciiMetadataValue, MetadataValue}, service::interceptor::InterceptedService, transport::channel::{Channel, ClientTlsConfig, Endpoint},
    }, yellowstone_grpc_client::{
        GeyserGrpcBuilder, GeyserGrpcBuilderError, GeyserGrpcClient, GeyserGrpcClientError, Interceptor, InterceptorXToken
    }, yellowstone_grpc_proto::{geyser::{SubscribeRequestFilterAccountsFilter, subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof,
    }, prelude::{
        subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterTransactions, SubscribeRequestAccountsDataSlice
    }}
};

use crate::error::MonitorError;

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

impl Bot {
    /// Monitors for new mints by subscribing to logs for the pump program
    pub async fn rpc_monitor_mints(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        info!("Starting mint monitoring...");
        info!("Using program ID: {}", PUMP_PROGRAM_ID.to_string());

        // Create rate limiter
        //WE are applying a 10 second rate limit for the Public RPC
        //We don't need this feature for the private RPC
        //TODO: Remove this rate limit for the private RPC
        //let last_request = Arc::new(Mutex::new(TokioInstant::now()));

        loop {
            if !*self.is_monitoring.lock().await {
                info!("RPC monitoring paused, breaking subscription loop");
                break Ok(());
            }
            info!("Attempting to establish WebSocket connection...");
            
            match self.ws_client.logs_subscribe(
                RpcTransactionLogsFilter::Mentions(vec![PUMP_PROGRAM_ID.to_string()]),
                RpcTransactionLogsConfig {
                    commitment: Some(CommitmentConfig::confirmed()),
                },
            ).await {
                Ok((mut subscription, _)) => {
                    info!("WebSocket subscription established successfully");
                    
                    while let Some(log_msg) = subscription.next().await {
                        // Check monitoring state on each message
                        if !*self.is_monitoring.lock().await {
                            info!("RPC monitoring paused, breaking subscription loop");
                            break;
                        }

                        info!("Received log message for signature: {}", log_msg.value.signature);
                        
                        let mut found_mint = false;
                        for log in log_msg.value.logs {
                            info!("Processing log: {}", log);
                            // Log program ID check
                            if log.contains(&PUMP_PROGRAM_ID.to_string()) {
                                debug!("Found target program ID in log");
                            }
                            
                            if !rpc_is_mint_log(&log) {
                                debug!("Not a mint log: {}", log);
                                continue;
                            }

                            info!("Detected Mint! Signature: {}, Log: {}", log_msg.value.signature, log);
                            found_mint = true;
                            
                            let bot = self.clone();
                            let signature = Signature::from_str(&log_msg.value.signature)?;
                            //let last_request = last_request.clone();
                            //WE are applying a 10 second rate limit for the Public RPC
                            //We don't need this feature for the private RPC
                            //TODO: Remove this rate limit for the private RPC
                            tokio::spawn(async move {
                                // Rate limit: ensure 10 second between requests
                                //let mut last = last_request.lock().await;
                                //let now = TokioInstant::now();
                                //let elapsed = now.duration_since(*last);
                                //if elapsed < Duration::from_secs(5) {
                                //    tokio::time::sleep(Duration::from_secs(5) - elapsed).await;
                                //}
                                //*last = TokioInstant::now();
                                
                                if let Err(e) = bot.rpc_check_and_signal_buy_coin(signature).await {
                                    error!("Error checking mint: {}", e);
                                }
                            });
                        }
                        
                        if !found_mint {
                            debug!("No mint detected in transaction {}", log_msg.value.signature);
                        }
                    }
                    error!("WebSocket subscription stream ended");
                },
                Err(e) => {
                    error!("Failed to establish WebSocket subscription. Error: {:?}", e);
                }
            }

            error!("WebSocket connection lost, retrying in 5 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    /// Checks if a new coin should be bought and handles it asynchronously
    async fn rpc_check_and_signal_buy_coin(&self, mint_sig: Signature) -> Result<(), Box<dyn Error + Send + Sync>> {
        let start = Instant::now();
        
        let new_coin = self.rpc_fetch_mint_details(mint_sig).await?;
        
        if !self.rpc_should_buy_coin(&new_coin).await? {
            return Ok(());
        }

        if start.elapsed().as_secs() > 1 {
            info!("Skipping {} (detail fetch took too long)", new_coin.mint_address.unwrap());
            return Ok(());
        }

        let mut coin = new_coin;
        coin.pickup_time = start;
        

        // Print the time when sending to buy channel
        let send_time = Utc::now();
        info!("Sent to buy channel at: {}", send_time.format("%Y-%m-%d %H:%M:%S%.3f"));
        
        
        // Send to buy channel
        let coin = Arc::new(Mutex::new(coin));
        if let Err(e) = self.coins_to_buy_sender.send(coin).await {
            error!("Failed to send coin to buy channel: {}", e);
        }
        
        Ok(())
    }

    /// Fetches details about a newly minted coin
    pub async fn rpc_fetch_mint_details(&self, sig: Signature) -> Result<Coin, Box<dyn Error + Send + Sync>> {
        //let mut retries = 3;
        //let mut last_error = None;
        //WE are applying a 10 second rate limit for the Public RPC
        //We don't need this feature for the private RPC
        //TODO: Remove this rate limit for the private RPC
        //while retries > 0 {
        info!("Attempting to fetch transaction {}", sig);
            
        match self.rpc_client.get_transaction_with_config(
            &sig,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(0),
            },
        ).await {
            Ok(tx) => {
                info!("Successfully fetched transaction {}", sig);
                
                // Print mint time if available
                if let Some(block_time) = tx.block_time {
                    let datetime = DateTime::from_timestamp(block_time, 0)
                        .unwrap_or_default();
                    info!("Mint time: {}", datetime);
                    info!("Mint Found at: {}",  Utc::now().format("%Y-%m-%d %H:%M:%S%.3f"));
                }
                
                // Handle both legacy and versioned transactions
                let transaction = match &tx.transaction.transaction {
                    solana_transaction_status::EncodedTransaction::Json(tx_json) => {
                        info!("Processing JSON transaction {}", sig);
                        match &tx_json.message {
                            solana_transaction_status::UiMessage::Raw(message) => {
                                // Convert JSON transaction to legacy transaction
                                let fee_payer = Pubkey::from_str(&message.account_keys[0])?;
                                
                                // Create message first
                                let mut legacy_message = solana_sdk::message::Message::new(
                                    &[],
                                    Some(&fee_payer)
                                );

                                // Set account keys
                                legacy_message.account_keys = message.account_keys.iter()
                                    .map(|key| Pubkey::from_str(key))
                                    .collect::<Result<Vec<_>, _>>()?;

                                // Set recent blockhash
                                legacy_message.recent_blockhash = message.recent_blockhash.parse()?;

                                // Set instructions
                                legacy_message.instructions = message.instructions.iter()
                                    .map(|ix| {
                                        solana_sdk::instruction::CompiledInstruction {
                                            program_id_index: ix.program_id_index,
                                            accounts: ix.accounts.clone(),
                                            data: solana_sdk::bs58::decode(&ix.data).into_vec().unwrap_or_default(),
                                        }
                                    })
                                    .collect();

                                info!("Reconstructed {} instructions", legacy_message.instructions.len());

                                Transaction {
                                    signatures: vec![sig],
                                    message: legacy_message,
                                }
                            },
                            _ => {
                                error!("Unsupported message format for transaction {}", sig);
                                return Err("Unsupported message format".into());
                            }
                        }
                    },
                    _ => {
                        error!("Unsupported transaction format for {}: {:?}", sig, tx.transaction.transaction);
                        return Err("Unsupported transaction format".into());
                    }
                };

                // Add logging for instruction processing
                info!("Checking for mint instructions in transaction {}", sig);
                // Extract coin details
                let mut new_coin = match rpc_fetch_new_coin(&transaction) {
                    Ok(coin) => coin,
                    Err(e) => {
                        error!("Failed to extract coin details from transaction {}: {}", sig, e);
                        return Err(e);
                    }
                };

                // Get creator buy information
                match rpc_fetch_creator_buy(&mut new_coin, &transaction) {
                    Ok(_) => {
                        info!("Successfully fetched mint details for signature {}", sig);
                        Ok(new_coin)
                    }
                    Err(e) => {
                        error!("Failed to fetch creator buy info for transaction {}: {}", sig, e);
                        Err(e)
                    }
                }
            },
            //  Err(e) => {
            //WE are applying a 10 second rate limit for the Public RPC
            //We don't need this feature for the private RPC
            //TODO: Remove this rate limit for the private RPC
            //error!("Failed to fetch mint transaction {} (retry {}): {}", sig, retries, e);
            //last_error = Some(e);
            //retries -= 1;
            //if retries > 0 {
            //    info!("Waiting 5s before retry...");
            //    tokio::time::sleep(Duration::from_secs(5)).await;
            //}
            Err(e) => {
                error!("Failed to fetch transaction {}: {}", sig, e);
                Err(Box::new(e))
            }
        }
        
        //error!("All retries exhausted for transaction {}", sig);
        //Err(Box::new(last_error.unwrap()))
    }

    /// Determines if a coin should be bought based on various criteria
    async fn rpc_should_buy_coin(&self, coin: &Coin) -> Result<bool, Box<dyn Error + Send + Sync>> {
        info!("Checking if coin should be bought");
        
        // Check price constraints
        let creator_pubkey = coin.creator.to_string();
        info!("Creator: {}", creator_pubkey);
        info!("Creator purchase amount: {} SOL", coin.creator_purchase_sol);
        
        if coin.creator_purchase_sol < 0.025 || coin.creator_purchase_sol > 2.5 {
            info!("Rejecting: Creator purchase amount {} SOL is outside allowed range (0.5-2.5)", coin.creator_purchase_sol);
            return Ok(false);
        }
        /* 
        // Make sure it's creator's first coin
        info!("Checking if creator has created coins before");
        if self.rpc_address_created_coin(&creator_pubkey).await? {
            info!("Rejecting: Creator has already created coins");
            return Ok(false);
        }
        info!("Creator has not created coins before");

        // Check 30 past transactions for funders
        info!("Fetching last 30 transactions for creator");
        let funder_trans = self.fetch_n_last_trans(30, &creator_pubkey).await?;
        info!("Found {} transactions", funder_trans.len());
        
        // Fetch up to 3 funders
        let creator_funders = find_funders_from_responses(&funder_trans, &creator_pubkey, 3);
        info!("Found {} funders", creator_funders.len());
        
        if creator_funders.is_empty() {
            info!("Rejecting: No funders found");
            return Ok(false);
        }
    

        let funders_len = creator_funders.len();
        let (tx, mut rx) = mpsc::channel(funders_len);
        
        info!("Checking {} funders for safety", funders_len);
        for funder in creator_funders {
            let bot = self.clone();
            let tx = tx.clone();
            let funder_addr = funder.clone();
            
            tokio::spawn(async move {
                let is_safe = bot.rpc_is_safe_funder(&funder).await;
                info!("Funder {} is safe: {}", funder_addr, is_safe);
                let _ = tx.send(is_safe).await;
            });
        }
          
        let mut safe_funders_count = 0;
        for _ in 0..funders_len {
            if rx.recv().await.unwrap_or(false) {
                safe_funders_count += 1;
                info!("Found safe funder {}/{}", safe_funders_count, funders_len);
            }
        }
          
        Ok(safe_funders_count == funders_len)
        */
        Ok(true)
    }

    /// Checks if a funder is considered safe
    async fn rpc_is_safe_funder(&self, funder: &str) -> bool {
        if is_exchange_address(funder) {
            return true;
        }

        match self.rpc_address_created_coin(funder).await {
            Ok(created) => !created,
            Err(_) => false,
        }
    }

    /// Checks if an address has created a coin before
    async fn rpc_address_created_coin(&self, creator_address: &str) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM coins WHERE creator_address = ?")
            .bind(creator_address)
            .fetch_one(&self.db_connection)
            .await?;

        Ok(count > 0)
    }
}

/// Extracts a new coin from a create instruction
fn rpc_fetch_new_coin(transaction: &Transaction) -> Result<Coin, Box<dyn Error + Send + Sync>> {
    info!("Attempting to extract coin details from transaction");
    info!("Number of instructions: {}", transaction.message.instructions.len());
    info!("Instructions: {:?}", transaction.message.instructions);
    info!("Account keys: {:?}", transaction.message.account_keys);
    // Directly access the instruction at index 2
    if let Some(instruction) = transaction.message.instructions.get(0) {
        info!("Processing instruction at index 2");

        let program_id = instruction.program_id(&transaction.message.account_keys);
        info!("Program ID: {}", program_id);

        if *program_id != PUMP_PROGRAM_ID {
            debug!("Skipping non-pump instruction");
            return Err(Box::new(MonitorError::CreatingNewCoin));
        }

        // Check instruction type and data length
        if instruction.data.len() < 8 {
            debug!("Instruction data too short: {}", instruction.data.len());
            return Err(Box::new(MonitorError::CreatingNewCoin));
        }

        let type_id = &instruction.data[0..8];
        info!("Instruction type ID: {:?}", type_id);
        info!("Expected CREATE ID: {:?}", INSTRUCTION_CREATE);

        // Only process Create instructions
        if type_id != INSTRUCTION_CREATE {
            debug!("Not a create instruction");
            return Err(Box::new(MonitorError::CreatingNewCoin));
        }
        info!("Found create instruction, here is the index: {}", instruction.program_id_index);
        
        // Decode instruction data
        info!("Attempting to decode create instruction data");
        let _create_data = CreateInstructionData::try_from_slice(&instruction.data[8..]).map_err(|e| {
            error!("Failed to decode Create instruction: {}", e);
            e
        })?;

        // Resolve instruction accounts
        info!("Attempting to resolve instruction accounts");
        info!("Account keys: {:?}", transaction.message.account_keys);
        info!("Instruction accounts: {:?}", instruction.accounts);

        let accounts = CreateAccounts::try_from(CreateInstructionWithAccounts {
            instruction,
            account_keys: &transaction.message.account_keys,
        }).map_err(|e| {
            error!("Failed to resolve instruction accounts: {}", e);
            e
        })?;

        // Create new coin from instruction accounts
        info!("Creating new coin from instruction accounts");
        return rpc_new_coin_from_create_inst(&accounts);
    }

    error!("No valid create instruction found in transaction");
    Err(Box::new(MonitorError::CreatingNewCoin))
}

/// Creates a new coin from a create instruction
fn rpc_new_coin_from_create_inst(accounts: &CreateAccounts) -> Result<Coin, Box<dyn Error + Send + Sync>> {
    // Validate all required accounts are present
    let mint_address = &accounts.mint;
    
    let bonding_curve = &accounts.bonding_curve;
    
    let associated_bonding_curve = &accounts.associated_bonding_curve;
    
    let event_authority = &accounts.event_authority;
    
    let creator = &accounts.user;
    //TODO: IT'S NOT SET THE OTHER FIELDS False/Positive
    // Create new coin with validated accounts
    Ok(Coin {
        mint_address: Some(mint_address.pubkey),
        token_bonding_curve: bonding_curve.pubkey,
        associated_bonding_curve: associated_bonding_curve.pubkey,
        event_authority: event_authority.pubkey,
        creator: creator.pubkey,
        ..Default::default()
    })
}

/// Extracts creator buy information from a transaction
fn rpc_fetch_creator_buy(coin: &mut Coin, transaction: &Transaction) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("Attempting to fetch creator buy information");
    info!("Number of instructions: {}", transaction.message.instructions.len());

    // Directly access the instruction at index 4
    if let Some(instruction) = transaction.message.instructions.get(2) {
        info!("Processing instruction at index 4");
        
        let program_id = instruction.program_id(&transaction.message.account_keys);
        info!("Program ID: {}", program_id);

        // Check program ID
        if *program_id != PUMP_PROGRAM_ID {
            debug!("Skipping non-pump instruction");
            return Err(Box::new(MonitorError::NoCreatorBuy));
        }

        // Check instruction type and data length
        if instruction.data.len() < 8 {
            debug!("Instruction data too short: {}", instruction.data.len());
            return Err(Box::new(MonitorError::NoCreatorBuy));
        }

        let type_id = &instruction.data[0..8];
        info!("Instruction type ID: {:?}", type_id);
        info!("Expected BUY ID: {:?}", INSTRUCTION_BUY);

        if type_id != INSTRUCTION_BUY {
            debug!("Not a buy instruction");
            return Err(Box::new(MonitorError::NoCreatorBuy));
        }

        info!("Found buy instruction, attempting to get accounts");
        info!("Account keys: {:?}", transaction.message.account_keys);
        info!("Instruction accounts: {:?}", instruction.accounts);

        // Get buy accounts
        let accounts = BuyAccounts::try_from_instruction(instruction, &transaction.message.account_keys).map_err(|_| {
            error!("Failed to get buy accounts");
            MonitorError::NoCreatorBuy
        })?;

        // Check associated token account
        if accounts.associated_user.pubkey == Pubkey::default() {
            error!("Invalid associated token account (default pubkey)");
            return Err(Box::new(MonitorError::NoCreatorATA));
        }

        info!("Successfully got buy accounts, decoding buy data");
        // Decode buy data
        let buy_data = BuyInstructionData::try_from_slice(&instruction.data[8..])
            .map_err(|e| {
                error!("Failed to decode buy data: {}", e);
                MonitorError::NoCreatorBuy
            })?;

        if buy_data.max_sol_cost == 0 {
            error!("Invalid buy amount (0 SOL)");
            return Err(Box::new(MonitorError::NoCreatorBuy));
        }

        info!("Successfully decoded buy data: {} SOL", buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64);
        coin.creator_purchased = true;
        coin.creator_purchase_sol = 0.99 * buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64;
        coin.creator_ata = Some(accounts.associated_user.pubkey);
        
        info!("Successfully updated coin with buy information");
        return Ok(());
    }

    error!("No valid buy instruction found in transaction");
    Err(Box::new(MonitorError::NoCreatorBuy))
}

/* 
/// Finds funders from transaction responses
fn find_funders_from_responses(responses: &[Transaction], creator_address: &str, funders_limit: usize) -> Vec<String> {
    info!("Searching for funders in {} transactions (limit: {})", responses.len(), funders_limit);
    let mut funders = Vec::new();
    let mut seen_funders = std::collections::HashSet::new();
    let mut seen_signatures = std::collections::HashSet::new();

    for (i, tx) in responses.iter().enumerate() {
        // Skip if we've seen this transaction before
        let sig = tx.signatures[0].to_string();
        if !seen_signatures.insert(sig.clone()) {
            info!("Skipping duplicate transaction: {}", sig);
            continue;
        }

        info!("Checking transaction {}/{} (signature: {})", i + 1, responses.len(), sig);
        if let Some(funder) = check_has_funder(tx, creator_address) {
            // Skip if we've seen this funder before
            if !seen_funders.insert(funder.clone()) {
                info!("Skipping duplicate funder: {}", funder);
                continue;
            }

            info!("Found new funder: {}", funder);
            funders.push(funder);
            
            if funders.len() == funders_limit {
                info!("Reached funders limit ({}), stopping search", funders_limit);
                break;
            }
        }
    }

    info!("Found {} unique funders from {} unique transactions", funders.len(), seen_signatures.len());
    funders
}



/// Checks if a transaction has a funder

fn check_has_funder(tx: &Transaction, creator_addr: &str) -> Option<String> {
    let has_only_micro_transfers = true;

    for (i, instruction) in tx.message.instructions.iter().enumerate() {
        debug!("Checking instruction {}", i);
        
        // Check if it's a system program instruction
        let program_id = instruction.program_id(&tx.message.account_keys);
        if *program_id != system_program::id() {
            debug!("Not a system program instruction");
            continue;
        }

        // First get the accounts for this instruction
        let accounts: Vec<Pubkey> = instruction.accounts.iter()
            .map(|&i| tx.message.account_keys[i as usize])
            .collect();

        // Try to decode as system instruction
        let system_instruction = match bincode::deserialize::<system_instruction::SystemInstruction>(&instruction.data) {
            Ok(si) => si,
            Err(e) => {
                debug!("Failed to decode system instruction: {}", e);
                continue;
            }
        };

        // Check if it's a transfer instruction
        if let system_instruction::SystemInstruction::Transfer { lamports } = system_instruction {
            // The funding account is the first account in the accounts list
            // This matches the system program's account ordering for transfers
            let funding_account = accounts[0].to_string();
            let sol_amount = lamports as f64 / LAMPORTS_PER_SOL as f64;
            
            // If we find a micro-transfer, skip the entire transaction
            if sol_amount <= 0.00001 {
                info!("Found micro-transfer ({} SOL) - skipping entire transaction", sol_amount);
                return None;
            }
            
            info!("Found SOL transfer: {} SOL from {}", sol_amount, funding_account);
            
            if funding_account != creator_addr && sol_amount > 0.05 {
                info!("Valid funder found: {} (transferred {} SOL)", funding_account, sol_amount);
                return Some(funding_account);
            } else {
                debug!("Transfer rejected: amount={} SOL (min 0.05), from_creator={}", 
                    sol_amount, funding_account == creator_addr);
            }
        } else {
            debug!("Not a transfer instruction");
        }
    }
    
    if has_only_micro_transfers {
        info!("Skipping transaction - contains only micro-transfers");
    } else {
        debug!("No valid funder found in transaction");
    }
    None
}
*/

/// Checks if a log entry is a mint log
fn rpc_is_mint_log(log_entry: &str) -> bool {
    // First check if this is a program invocation of our target program
    if log_entry.contains(&format!("Program {} invoke", PUMP_PROGRAM_ID.to_string())) {
        debug!("Found program invocation");
        return false;
    }

    // Exact match for Create instruction - nothing else
    if log_entry == "Program log: Instruction: Create" {
        debug!("Found Create instruction: {}", log_entry);
    
        return true;
    }

    false
 
}

