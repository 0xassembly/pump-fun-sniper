use {
    crate::{
        bot::{structs::Coin, utils::is_exchange_address, Bot, constants::{LAMPORTS_PER_SOL, INSTRUCTION_BUY, INSTRUCTION_CREATE, PROGRAM_ID as PUMP_PROGRAM_ID, PUMP_METAPLEX_ID, MINT_AUTH_ID}},
        pump::{
            buy::{BuyAccounts,BuyInstructionData}, create::{CreateAccounts, CreateInstructionData, CreateInstructionWithAccounts},
        },
        error::MonitorError,
    }, 
    bincode, 
    borsh::{BorshDeserialize, BorshSerialize}, 
    chrono::{DateTime, Utc}, 
    futures::{Sink, SinkExt, Stream, StreamExt}, 
    log::{debug, error, info}, 
    solana_client::{
        nonblocking::{
            pubsub_client::{PubsubClient, PubsubClientError}, 
            rpc_client::RpcClient
        },
        pubsub_client::PubsubClientSubscription,
        rpc_config::{RpcTransactionConfig, RpcTransactionLogsConfig, RpcTransactionLogsFilter}
    }, 
    solana_program::{
        instruction::Instruction,
        program_pack::Pack,
        pubkey::Pubkey,
        system_instruction,
        system_program
    }, 
    solana_sdk::{
        bs58, commitment_config::CommitmentConfig, inner_instruction, signature::Signature, transaction::Transaction
    }, 
    solana_transaction_status::UiTransactionEncoding, 
    std::{
        any::Any, collections::HashMap, error::Error, str::FromStr, sync::Arc, time::{Duration, Instant}
    }, 
    tokio::{
        sync::{mpsc::{self, error::SendError, UnboundedReceiver}, Mutex}, 
        time::Instant as TokioInstant
    }, 
    tonic::{
        codec::{CompressionEncoding, Streaming}, 
        metadata::{errors::InvalidMetadataValue, AsciiMetadataValue, MetadataValue}, 
        service::interceptor::InterceptedService, 
        transport::channel::{Channel, ClientTlsConfig, Endpoint}, 
        Status
    }, 
    yellowstone_grpc_client::{
        GeyserGrpcBuilder, 
        GeyserGrpcBuilderError, 
        GeyserGrpcClient, 
        GeyserGrpcClientError, 
        Interceptor, 
        InterceptorXToken
    }, 
    yellowstone_grpc_proto::{
        geyser::{
            subscribe_request_filter_accounts_filter::Filter as AccountsFilterDataOneof, SubscribeRequestFilterAccountsFilter
        }, prelude::{
            subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestAccountsDataSlice, SubscribeRequestFilterAccounts, SubscribeRequestFilterBlocksMeta, SubscribeRequestFilterTransactions, SubscribeUpdate, SubscribeUpdateTransactionInfo
        }
    },
};


// Module-level utility functions
fn grpc_fetch_new_coin(transaction: &SubscribeUpdateTransactionInfo) -> Result<Coin, Box<dyn Error + Send + Sync>> {
    let message_clone = &transaction.transaction.as_ref().unwrap().message.as_ref().unwrap().clone();
    info!("Attempting to extract coin details from transaction");
   
    // Check all instructions starting from index 2
    for i in 2..message_clone.instructions.len() {
        if let Some(instruction) = &message_clone.instructions.get(i) {
            if instruction.data.len() < 8 {
                error!("Instruction data too short: {}", instruction.data.len());
                continue;
            }
            let type_id = &instruction.data[0..8];
            //info!("Instruction type ID: {:?}", type_id);
            //info!("Expected CREATE ID: {:?}", INSTRUCTION_CREATE);

            // Only process Create instructions
            if type_id == INSTRUCTION_CREATE {
                grpc_decode_token_metadata(&instruction.data);
                
                let solana_instruction = solana_sdk::instruction::CompiledInstruction {
                    program_id_index: instruction.program_id_index as u8,
                    accounts: instruction.accounts.clone(),
                    data: instruction.data.clone(),
                };

                let account_keys: &Vec<Pubkey> = &message_clone.account_keys.iter()
                    .map(|key| Pubkey::from_str(&bs58::encode(key).into_string()))
                    .collect::<Result<Vec<_>, _>>()?;

                let accounts = CreateAccounts::try_from(CreateInstructionWithAccounts {
                    instruction: &solana_instruction,
                    account_keys: account_keys,
                }).map_err(|e| {
                    error!("Failed to resolve instruction accounts: {}", e);
                    e
                })?;
                
                // Create new coin from instruction accounts
                info!("Creating new coin from instruction accounts");
                return grpc_new_coin_from_create_inst(&accounts);
            }
        }
    }

    debug!("No create instruction found in transaction");
    return Err(Box::new(MonitorError::NoCreateInstruction));
}

fn grpc_decode_token_metadata(metadata: &Vec<u8>) -> CreateInstructionData {
    // Skip first few bytes (instruction discriminator)
    let name_len = u32::from_le_bytes(metadata[8..12].try_into().unwrap()) as usize;
    let name_start = 12;
    let name_end = name_start + name_len;
    let name = String::from_utf8_lossy(&metadata[name_start..name_end]);
    
    let symbol_len = u32::from_le_bytes(metadata[name_end..name_end+4].try_into().unwrap()) as usize;
    let symbol_start = name_end + 4;
    let symbol_end = symbol_start + symbol_len;
    let symbol = String::from_utf8_lossy(&metadata[symbol_start..symbol_end]);
    
    let uri_len = u32::from_le_bytes(metadata[symbol_end..symbol_end+4].try_into().unwrap()) as usize;
    let uri_start = symbol_end + 4;
    let uri_end = uri_start + uri_len;
    let uri = String::from_utf8_lossy(&metadata[uri_start..uri_end]);


    CreateInstructionData {
        name: name.to_string()  ,
        symbol: symbol.to_string(),
        uri: uri.to_string(),
    }

}

fn grpc_new_coin_from_create_inst(accounts: &CreateAccounts) -> Result<Coin, Box<dyn Error + Send + Sync>> {
    // Validate all required accounts are present
    let mint_address = &accounts.mint;
    
    let bonding_curve = &accounts.bonding_curve;
    
    let associated_bonding_curve = &accounts.associated_bonding_curve;
    
    let event_authority = &accounts.event_authority;
    
    let creator = &accounts.user;
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

fn grpc_decode_event_data(coin: &mut Coin, transaction: &SubscribeUpdateTransactionInfo) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Some(meta) = &transaction.meta {
        let inner_instructions = meta.inner_instructions.clone();
        //info!("Inner instructions: {:?}", inner_instructions);
        const EVENT_DISCRIMINATOR: [u8; 8] = [228, 69, 165, 46, 81, 203, 154, 29];

        for inner_ix_set in inner_instructions {
            for inner_ix in inner_ix_set.instructions {
                // Check instruction data
                if inner_ix.data.len() >= 130  // Ensure data is long enough (8 + 2 bytes minimum)
                    && inner_ix.data[0..8] == EVENT_DISCRIMINATOR  // Check start
                    && inner_ix.data[inner_ix.data.len()-2..] == [2, 0]  // Check end
                {
                    //info!("INNER INSTRUCTION: {:?} ", inner_ix.data);
                
                    let offset = 8; // Skip the first 8 bytes (discriminator)
                    /* 
                    // Decode mint (PublicKey)
                    let mint = bs58::encode(&inner_ix.data[offset + 8 ..offset + 40]).into_string();
                    
                    // Decode solAmount (u64)
                    let sol_amount = u64::from_le_bytes(inner_ix.data[offset + 40 ..offset + 48].try_into()?);

                    // Decode tokenAmount (u64)
                    let token_amount = u64::from_le_bytes(inner_ix.data[offset + 48..offset + 56].try_into()?);
             
                
                    // Decode isBuy (bool)
                    let is_buy = inner_ix.data[offset+56] != 0; // 0 = false, non-zero = true
                    
                    // Decode user (PublicKey)
                    let user = bs58::encode(&inner_ix.data[offset + 57..offset + 89]).into_string();
                    */
                    // Decode timestamp (i64)
                    let timestamp_bytes: [u8; 8] = inner_ix.data[offset + 89..offset + 97].try_into()?;
                    let timestamp = i64::from_le_bytes(timestamp_bytes);
                    coin.mint_timestamp = timestamp;
                    // Decode virtualSolReserves (u64)
                    let virtual_sol_reserves = u64::from_le_bytes(inner_ix.data[offset + 97..offset + 105].try_into()?);
                    coin.bonding_curve_data.as_mut().unwrap().virtual_sol_reserves = virtual_sol_reserves as u128;
                    // Decode virtualTokenReserves (u64)
                    let virtual_token_reserves = u64::from_le_bytes(inner_ix.data[offset + 105..offset + 113].try_into()?);
                    coin.bonding_curve_data.as_mut().unwrap().virtual_token_reserves = virtual_token_reserves as u128;
                    info!("DECODED TIMESTAMP: {}", timestamp);
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

fn grpc_process_buy_instruction(coin: &mut Coin, message_clone: &yellowstone_grpc_proto::prelude::Message) -> Result<(), Box<dyn Error + Send + Sync>> {
    for i in 2..message_clone.instructions.len() {
        if let Some(instruction) = message_clone.instructions.get(i) {
            info!("Processing instruction at index {}", i);
            
            // Check instruction data length first
            if instruction.data.len() < 8 {
                debug!("Instruction data too short at index {}: {}", i, instruction.data.len());
                continue;
            }

            let type_id = &instruction.data[0..8];
            if type_id != INSTRUCTION_BUY {
                continue;
            }


            //info!("Instruction type ID: {:?}", type_id);
            //info!("Expected BUY ID: {:?}", INSTRUCTION_BUY);

            info!("Found buy instruction, attempting to get accounts");
            //info!("Account keys: {:?}", message_clone.account_keys);
            //info!("Instruction accounts: {:?}", instruction.accounts);

            let solana_instruction = solana_sdk::instruction::CompiledInstruction {
                program_id_index: instruction.program_id_index as u8,
                accounts: instruction.accounts.clone(),
                data: instruction.data.clone(),
            };

            let account_keys: &Vec<Pubkey> = &message_clone.account_keys.iter()
            .map(|key| Pubkey::from_str(&bs58::encode(key).into_string()))
            .collect::<Result<Vec<_>, _>>()?;

            // Get buy accounts
            let accounts = BuyAccounts::try_from_instruction(&solana_instruction, account_keys).map_err(|_| {
                error!("Failed to get buy accounts");
                MonitorError::BuyAccountsCantDecode
            })?;

            // Check associated token account
            if accounts.associated_user.pubkey == Pubkey::default() {
                error!("Invalid associated token account (default pubkey)");
                continue;
            }

            let buy_data = BuyInstructionData::try_from_slice(&solana_instruction.data[8..])
                .map_err(|_| MonitorError::BuyDataCantDecode)?;

            if buy_data.max_sol_cost == 0 {
                return Err(Box::new(MonitorError::ZeroBuyAmount));
            }

            info!("Successfully decoded buy data: {} SOL", buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64);
            coin.creator_purchased = true;
            coin.creator_purchase_sol = 0.99 * buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64;
            coin.creator_ata = Some(accounts.associated_user.pubkey);
            
            info!("Successfully updated coin with buy information");
            return Ok(());
        }
    }

    error!("No valid buy instruction found in transaction");
    Err(Box::new(MonitorError::NoCreatorBuy))
}

fn grpc_fetch_creator_buy(coin: &mut Coin, transaction: &SubscribeUpdateTransactionInfo) -> Result<(), Box<dyn Error + Send + Sync>> {
    let message_clone = &transaction.transaction.as_ref().unwrap().message.as_ref().unwrap().clone();

    grpc_decode_event_data(coin, transaction)?;
    grpc_process_buy_instruction(coin, message_clone)
}

impl Bot {
    pub async fn grpc_monitor_mints(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut grpc_client = match self.grpc_connect_client().await {
                Ok(client) => client,
                Err(err) => {
                    error!("Error connecting to gRPC client: {}", err);
                return Err(err);
                }
            };

            // Set up subscription
            let (mut subscribe_tx, mut stream) = match grpc_client.subscribe().await {
                Ok(sub) => sub,
                Err(err) => {
                    error!("Error subscribing to gRPC: {}", err);
                return Err(Box::new(err));
            }
        };
        
        loop {
            // Check monitoring state before attempting to connect
            if !*self.is_monitoring.lock().await {
                info!("GRPC monitoring paused, waiting...");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }

            if let Err(err) = subscribe_tx.send(self.grpc_create_subscription_request()).await {
                error!("Error sending subscription request: {}", err);
                continue;
            }

            self.grpc_process_stream(&mut stream).await;
            
            error!("Stream disconnected, reconnecting...");
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn grpc_connect_client(&self) -> Result<GeyserGrpcClient<impl Interceptor>, Box<dyn Error + Send + Sync>> {
        info!("Connecting gRPC client");
        let client = GeyserGrpcClient::build_from_shared(self.geyser_endpoint.to_string())?
            .x_token(self.geyser_x_token.clone())?
            .tls_config(ClientTlsConfig::new().with_native_roots())?
            .max_decoding_message_size(1024 * 100)
            .connect()
            .await?;
        
        info!("Connected to gRPC");
        Ok(client)
    }

    fn grpc_create_subscription_request(&self) -> SubscribeRequest {
        let from_slot = 0;
        SubscribeRequest {
            accounts: HashMap::from([
                (PUMP_PROGRAM_ID.to_string(), SubscribeRequestFilterAccounts {
                    account: vec![self.fee_id.to_string(), MINT_AUTH_ID.to_string(), PUMP_METAPLEX_ID.to_string()],
                    owner: vec![PUMP_PROGRAM_ID.to_string()],
                    filters: vec![],
                    nonempty_txn_signature: Some(false),
                })
            ]),
            slots: HashMap::new(),
            transactions_status: HashMap::new(),
            transactions: HashMap::from([
                ("pump_txs".to_string(), SubscribeRequestFilterTransactions {
                    vote: Some(false),
                    failed: Some(false),
                    signature: None,
                    account_include: vec![PUMP_PROGRAM_ID.to_string()],
                    account_exclude: vec![],
                    account_required: vec![self.fee_id.to_string(), MINT_AUTH_ID.to_string(), PUMP_METAPLEX_ID.to_string()],
                })
            ]),
            blocks: HashMap::new(),
            blocks_meta: HashMap::new(),
            entry: HashMap::new(),
            commitment: Some(CommitmentLevel::Processed as i32),
            accounts_data_slice: vec![],
            ping: None,
            from_slot: Some(from_slot),
        }
    }


    async fn grpc_process_stream(&self, stream: &mut (impl Stream<Item = Result<SubscribeUpdate, Status>> + Unpin)) {
        while let Some(message) = stream.next().await {
            // Check monitoring state on each message
            if !*self.is_monitoring.lock().await {
                info!("GRPC monitoring stopped");
                return;
            }
            match message {
                Ok(msg) => {
                    if let Some(UpdateOneof::Transaction(tx)) = msg.update_oneof {
                        if let Some(meta) = tx.transaction {
                            self.grpc_process_transaction(meta, tx.slot).await;
                        }
                    }
                }
                Err(error) => {
                    error!("Stream error: {:?}", error);
                    break;
                }
            }
        }
    }

    async fn grpc_process_transaction(&self, meta: SubscribeUpdateTransactionInfo, slot: u64) -> Result<(), Box<dyn Error + Send + Sync>> {
        let meta_clone = &meta.clone();
        if let Some(meta_info) = meta_clone.clone().meta {
            for log in meta_info.log_messages {
                //info!("Log message: {}", log);
                
                if log == "Program log: Instruction: Create" {
                    if let Err(e) = self.grpc_handle_create_instruction(meta_clone.clone(), slot).await {
                        error!("Failed to handle create instruction: {}", e);
                    }
                    if let Err(e) = self.grpc_check_and_signal_buy_coin(meta_clone.clone()).await {
                        error!("Failed to check and signal buy coin: {}", e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn grpc_handle_create_instruction(&self, transaction: SubscribeUpdateTransactionInfo, slot: u64)  -> Result<(), Box<dyn Error + Send + Sync>> {
        let mint_found_at = Utc::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string();
        let signature = bs58::encode(&transaction.signature).into_string();
        
        //self.grpc_spawn_transaction_monitor(signature.clone(), mint_found_at).await;
        
        if let Some(transaction_info) = transaction.meta {
            info!("Compute units: {:?}", transaction_info.compute_units_consumed);
        }
        info!("Slot: {}", slot);
        info!("Signature: {}", signature);
        Ok(())
    }

    /// Checks if a new coin should be bought and handles it asynchronously
    async fn grpc_check_and_signal_buy_coin(&self, transaction: SubscribeUpdateTransactionInfo) -> Result<(), Box<dyn Error + Send + Sync>> {
        let start = Instant::now();
        
        let new_coin = self.grpc_fetch_mint_details(transaction).await?;
        
        if !self.grpc_should_buy_coin(&new_coin).await? {
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
    pub async fn grpc_fetch_mint_details(&self, transaction: SubscribeUpdateTransactionInfo) -> Result<Coin, Box<dyn Error + Send + Sync>> {
        let transaction_clone = transaction.clone();
        let sig_clone = bs58::encode(&transaction_clone.signature).into_string();
        let message_clone = &transaction_clone.transaction.as_ref().unwrap().message.as_ref().unwrap().clone();
        info!("Attempting to fetch transaction {:?}", &sig_clone);
            

                info!("Successfully fetched transaction {:?}", &sig_clone);
                
                
                let fee_payer = Pubkey::from_str(&bs58::encode(&message_clone.account_keys[0]).into_string())?;
                info!("Fee Payer: {}", fee_payer);
                                
                                // Create message first
                                let mut legacy_message = solana_sdk::message::Message::new(
                                    &[],
                                    Some(&fee_payer)
                                );
                

                                // Set account keys
                legacy_message.account_keys = message_clone.account_keys.iter()
                    .map(|key| Pubkey::from_str(&bs58::encode(key).into_string()))
                                    .collect::<Result<Vec<_>, _>>()?;

                //info!("Acount keys: {:?}", legacy_message.account_keys);

                                // Set recent blockhash
                legacy_message.recent_blockhash = solana_sdk::hash::Hash::from_str(&bs58::encode(&message_clone.recent_blockhash).into_string())?;
                info!("Recent Blockhash: {:?}", legacy_message.recent_blockhash);
                *self.blockhash.lock().await = Some(legacy_message.recent_blockhash);

                                // Set instructions
                legacy_message.instructions = message_clone.instructions.iter()
                                    .map(|ix| {
                        //info!("Original instruction data: {:?}", ix.data);
                                        solana_sdk::instruction::CompiledInstruction {
                            program_id_index: ix.program_id_index as u8,
                                            accounts: ix.accounts.clone(),
                            data: ix.data.clone(),
                                        }
                                    })
                                    .collect();

                //info!("Reconstructed {:?} instructions", legacy_message.instructions);

                // Add logging for instruction processing
                info!("Checking for mint instructions in transaction {}", &sig_clone);
                // Extract coin details

                let mut new_coin = match grpc_fetch_new_coin(&transaction_clone) {
                    Ok(coin) => coin,
                    Err(e) => {
                        error!("Failed to extract coin details from transaction {}: {}", &sig_clone, e);
                        return Err(e);
                    }
                };
                

                // Get creator buy information
                match grpc_fetch_creator_buy(&mut new_coin, &transaction_clone) {
                    Ok(_) => {
                        info!("Successfully fetched mint details for signature {}", &sig_clone);
                        Ok(new_coin)
                    }
                    Err(e) => {
                        error!("Failed to fetch creator buy info for transaction {}: {}", &sig_clone, e);
                        Err(e)
                    }
                }

        }
        


    /// Determines if a coin should be bought based on various criteria
    async fn grpc_should_buy_coin(&self, coin: &Coin) -> Result<bool, Box<dyn Error + Send + Sync>> {
        info!("Checking if coin should be bought");
        info!("Check The time");
        
        if coin.mint_timestamp < (Utc::now().timestamp()) {
            error!("Mint timestamp is too old");
            return Ok(false);
        }
        // Check price constraints
        let creator_pubkey = coin.creator.to_string();
        info!("Mint timestamp: {}", coin.mint_timestamp);
        info!("Current timestamp: {}", Utc::now().timestamp());
        info!("Creator: {}", creator_pubkey);
        info!("Creator purchase amount: {} SOL", coin.creator_purchase_sol);
        //coin.creator_purchase_sol < 0.2 || coin.creator_purchase_sol > 2.5 
        if coin.creator_purchase_sol > 1.5 || coin.creator_purchase_sol < 0.2{
            info!("Rejecting: Creator purchase amount {} SOL is outside allowed range (0.5-1.0)", coin.creator_purchase_sol);
            return Ok(false);
        }
       
        Ok(true)
    }


}


