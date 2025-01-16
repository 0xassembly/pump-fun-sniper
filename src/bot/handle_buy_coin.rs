use {
    crate::bot::{Bot, structs::Coin},
    log::{error, info},
    solana_client::{
        nonblocking::{
            rpc_client::RpcClient,
            pubsub_client::{PubsubClient, PubsubClientError},
        },
        rpc_config::{RpcTransactionConfig, RpcAccountInfoConfig},
        rpc_client::GetConfirmedSignaturesForAddress2Config,
    },
    solana_program::{
        pubkey::Pubkey,
        instruction::AccountMeta,
    },
    solana_sdk::{
        commitment_config::CommitmentConfig,
        signature::Signature,
        bs58,
    },
    solana_transaction_status::{
        EncodedConfirmedTransactionWithStatusMeta,
        UiTransactionEncoding,
        UiMessage,
        UiInstruction,
        EncodedTransaction,
        option_serializer::OptionSerializer,
    },
    std::{
        sync::Arc,
        time::Duration,
        str::FromStr,
        error::Error as StdError,
    },
    tokio::{
        sync::{mpsc, Mutex},
        time::sleep,
    },
    futures::stream::StreamExt,
    crate::pump,
    spl_token::instruction::TokenInstruction,
    anyhow::{Result, Error, anyhow},
};

impl Bot {
    // Main handler that processes coins to buy from the channel
    pub async fn handle_buy_coins(&self, should_geyser: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("Starting buy coins handler...");
        while let Some(coin) = self.coins_to_buy.lock().await.recv().await {
            // Check monitoring state before processing new coin
            self.check_monitoring_state().await;
            
            info!("Received coin to buy: {:?}", coin.lock().await.mint_address);
            let bot = self.clone();
            
            // Spawn the task and get its handle
            let handle = tokio::spawn(async move {
                info!("Starting purchase for coin: {:?}", coin.lock().await.mint_address);
                match bot.purchase_coin(coin.clone(), should_geyser).await {
                    Ok(_) => {
                        info!("Successfully purchased coin");
                        Ok(())
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        error!("Error in purchase_coin: {}", err_msg);
                        Err(e)
                    }
                }
            });

            // Handle any errors from the spawned task
            tokio::spawn(async move {
                match handle.await {
                    Ok(result) => {
                        if let Err(e) = result {
                            error!("Purchase task failed: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Purchase task panicked: {}", e);
                    }
                }
            });
        }
        error!("Buy coins channel closed unexpectedly");
        Ok(())
    }

    async fn handle_purchase_error(&self, e: Box<dyn std::error::Error + Send + Sync>, sender: &mpsc::Sender<String>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let err_msg = e.to_string();
        error!("Error Buying Coin: {}", &err_msg);
        
        let status_msg = format!("Error Buying Coin: {}", err_msg);
        if let Err(send_err) = sender.send(status_msg.clone()).await {
            let send_err_msg = send_err.to_string();
            error!("Failed to send status: {}", send_err_msg);
        }
        
        Err(e)
    }

    async fn purchase_coin(&self, coin: Arc<Mutex<Coin>>, should_geyser: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.buy_coin(coin.clone(), should_geyser).await?;
        self.add_new_pending_coin(coin.clone()).await;

        Ok(())
    }

    async fn add_new_pending_coin(&self, coin: Arc<Mutex<Coin>>) {
        let coin_guard = coin.lock().await;
        let mint_addr = coin_guard.mint_address.unwrap().to_string();
        drop(coin_guard); // Explicitly drop before acquiring next lock

        let mut pending_coins = self.pending_coins.lock().await;
        pending_coins.insert(mint_addr, coin);
    }

    async fn listen_creator_sell(&self, coin: Arc<Mutex<Coin>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Get creator ATA
        let creator_ata = {
            let coin_guard = coin.lock().await;
            match coin_guard.creator_ata {
                Some(ata) => ata,
                None => {
                    self.set_creator_sold(coin.clone()).await;
                    return Ok(());
                }
            }
        };

        // Create account subscription using the existing ws_client
        let subscription_result = self.ws_client.account_subscribe(
            &creator_ata,
            Some(RpcAccountInfoConfig {
                encoding: None,
                data_slice: None,
                commitment: Some(CommitmentConfig::confirmed()),
                min_context_slot: None,
            }),
        ).await;

        let (mut subscription, _unsubscribe) = match subscription_result {
            Ok(sub) => sub,
            Err(e) => return Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
        };

        while let Some(update) = subscription.next().await {
            // Check if we should exit
            {
                let coin_guard = coin.lock().await;
                if (coin_guard.exited_buy_coin && !coin_guard.bot_purchased)
                    || (coin_guard.bot_purchased && !coin_guard.bot_holds_tokens())
                {
                    info!("No buy recorded or bot already sold tokens, stopping listener");
                    break;
                }
            }

            // Make multiple attempts to check for sells/transfers
            let mut detected_activity = false;
            for _ in 0..3 {
                match self.fetch_creator_ata_trans(&creator_ata).await {
                    Ok(transactions) => {
                        if self.is_sell_or_transfer(&transactions, &creator_ata).await {
                            info!("Detected Sale / Transfer for {}", creator_ata);
                            self.set_creator_sold(coin.clone()).await;
                            detected_activity = true;
                            break;
                        }
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        error!("Error fetching creator transactions: {}", err_msg);
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            if detected_activity {
                break;
            }
        }

        // Set exited_creator_listener at the end
        let mut coin_guard = coin.lock().await;
        coin_guard.exited_creator_listener = true;
        Ok(())
    }

    async fn set_creator_sold(&self, coin: Arc<Mutex<Coin>>) {
        let mut pending_coins = self.pending_coins.lock().await;
        let coin_guard = coin.lock().await;
        let mint_addr = coin_guard.mint_address.unwrap().to_string();
        if let Some(coin) = pending_coins.get_mut(&mint_addr) {
            let mut coin_guard = coin.lock().await;
            coin_guard.creator_sold = true;
        }
    }

    async fn fetch_creator_ata_trans(&self, creator_ata: &Pubkey) -> Result<Vec<EncodedConfirmedTransactionWithStatusMeta>, Box<dyn std::error::Error + Send + Sync>> {
        // Get last 3 signatures with one RPC call
        let config = GetConfirmedSignaturesForAddress2Config {
            before: None,
            until: None,
            limit: Some(3),
            commitment: Some(CommitmentConfig::confirmed()),
        };

        let signatures = self.rpc_client
            .get_signatures_for_address_with_config(
                creator_ata,
                config,
            )
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

        // Get transactions in parallel using futures
        let signatures: Vec<_> = signatures
            .iter()
            .filter_map(|sig| Signature::from_str(&sig.signature).ok())
            .collect();

        let transaction_futures: Vec<_> = signatures
            .iter()
            .map(|signature| {
                self.rpc_client.get_transaction_with_config(
                    signature,
                    RpcTransactionConfig {
                        encoding: Some(UiTransactionEncoding::Json),
                        commitment: Some(CommitmentConfig::confirmed()),
                        max_supported_transaction_version: Some(0),
                    },
                )
            })
            .collect();

        let transactions = futures::future::join_all(transaction_futures)
            .await
            .into_iter()
            .filter_map(|result| result.ok())
            .collect();

        Ok(transactions)
    }

    async fn is_sell_or_transfer(&self, transactions: &[EncodedConfirmedTransactionWithStatusMeta], creator_ata: &Pubkey) -> bool {
        for tx in transactions {
            if self.detect_transfer(tx, creator_ata).await || self.detect_sell(tx).await {
                return true;
            }
        }
        false
    }

    async fn detect_sell(&self, transaction: &EncodedConfirmedTransactionWithStatusMeta) -> bool {
        if let Some(meta) = &transaction.transaction.meta {
            // Check each instruction in the transaction
            if let EncodedTransaction::Json(tx) = &transaction.transaction.transaction {
                if let UiMessage::Raw(message) = &tx.message {
                    for instruction in &message.instructions {
                        // Get accounts for this instruction
                        let accounts: Vec<Pubkey> = instruction.accounts.iter()
                            .filter_map(|&i| message.account_keys
                                .get(i as usize)
                                .and_then(|s| Pubkey::from_str(s).ok()))
                            .collect();

                        // Try to decode the instruction
                        if let Ok(decoded_ix) = pump::decode_instruction(
                            &instruction.accounts.iter()
                                .map(|&i| AccountMeta::new_readonly(
                                    Pubkey::from_str(message.account_keys[i as usize].as_str()).unwrap(),
                                    false
                                ))
                                .collect::<Vec<_>>(), 
                            &bs58::decode(&instruction.data).into_vec().unwrap()
                        ) {
                            // Get instruction data
                            if let Ok(data) = decoded_ix.data() {
                                if data.len() >= 8 {
                                    let type_id = &data[0..8];
                                    
                                    // Check against known pump instruction IDs
                                    for (id, v) in pump::PUMP_IDS.iter() {
                                        if id == type_id && v.name == "sell" {
                                            info!("*** Found a sell in the decodedInstructions");
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }

    async fn detect_transfer(&self, transaction: &EncodedConfirmedTransactionWithStatusMeta, creator_ata: &Pubkey) -> bool {
        if let Some(meta) = &transaction.transaction.meta {
            // Check inner instructions
            if let OptionSerializer::Some(inner_instructions) = &meta.inner_instructions {
                for inner_instruction in inner_instructions {
                    for instruction in &inner_instruction.instructions {
                        if let UiInstruction::Compiled(instruction) = instruction {
                            // Get program ID for this instruction
                            if let EncodedTransaction::Json(tx) = &transaction.transaction.transaction {
                                if let UiMessage::Raw(message) = &tx.message {
                                    let program_id = if let Some(key) = message.account_keys
                                        .get(instruction.program_id_index as usize)
                                        .and_then(|s| Pubkey::from_str(s).ok()) {
                                        key
                                    } else {
                                        continue;
                                    };

                                    // Check if it's a token program instruction
                                    if program_id == spl_token::id() {
                                        // Get accounts for this instruction
                                        let accounts: Vec<Pubkey> = instruction.accounts.iter()
                                            .filter_map(|&i| message.account_keys
                                                .get(i as usize)
                                                .and_then(|s| Pubkey::from_str(s).ok()))
                                            .collect();

                                        // Try to decode the token instruction
                                        if let Ok(token_ix) = spl_token::instruction::TokenInstruction::unpack(&bs58::decode(&instruction.data).into_vec().unwrap()) {
                                            match token_ix {
                                                TokenInstruction::Transfer { amount: _ } => {
                                                    // Check if the source account is the creator's ATA
                                                    if let Some(source_account) = accounts.first() {
                                                        if source_account == creator_ata {
                                                            return true;
                                                        }
                                                    }
                                                }
                                                _ => continue,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        false
    }
} 