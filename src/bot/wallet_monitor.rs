use {
    super::constants::EVENT_AUTH_ID, crate::{bot::{constants::{INSTRUCTION_BUY, INSTRUCTION_SELL, MINT_AUTH_ID, PROGRAM_ID as PUMP_PROGRAM_ID, PUMP_METAPLEX_ID}, grpc_monitor_mints::MonitorError, structs::Coin, Bot}, pump::{buy::{BuyAccounts, BuyInstructionData}, sell::{SellAccounts, SellInstructionData}}}, borsh::BorshDeserialize, futures::{SinkExt, Stream, StreamExt}, log::{error, info}, solana_program::pubkey::Pubkey, solana_sdk::bs58, std::{collections::HashMap, error::Error, str::FromStr, sync::Arc}, tokio::{sync::Mutex, time::Duration}, tonic::Status, yellowstone_grpc_proto::prelude::{
        subscribe_update:: UpdateOneof, CommitmentLevel, SubscribeRequest, SubscribeRequestFilterAccounts, SubscribeRequestFilterTransactions, SubscribeUpdate, SubscribeUpdateTransactionInfo
    }

};

lazy_static::lazy_static! {
    static ref WALLET_ADDRESS: String = String::from("DfMxre4cKmvogbLrPigxmibVTTQDuzjdXojWzjCXXhzj");
    static ref WALLET_BUY_DISCRIMINATOR: [u8; 8] = [82, 225, 119, 231, 78, 29, 45, 70];
    static ref WALLET_SELL_DISCRIMINATOR: [u8; 8] = [93, 88, 60, 34, 91, 18, 86, 197];
    static ref EVENT_DISCRIMINATOR: [u8; 8] = [228, 69, 165, 46, 81, 203, 154, 29];
}

const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
impl Bot {

    pub async fn grpc_monitor_wallet(&self) -> Result<(), Box<dyn Error + Send + Sync>> {
        loop {
        let mut grpc_client = match self.grpc_connect_client().await {
            Ok(client) => client,
            Err(err) => {
                error!("Error connecting to gRPC client: {}", err);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };


        // Set up subscription
        let (mut subscribe_tx, mut stream) = match grpc_client.subscribe().await {
            Ok(sub) => sub,
            Err(err) => {
                error!("Error subscribing to gRPC: {}", err);
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };


        if let Err(err) = subscribe_tx.send(self.create_wallet_monitor_request(&WALLET_ADDRESS)).await {
            error!("Error sending subscription request: {}", err);
            continue;
        }

        self.monitor_wallet_stream(&mut stream).await;
        }
    }

    fn create_wallet_monitor_request(&self, wallet: &str) -> SubscribeRequest {
        let target_wallet = Pubkey::from_str(wallet).expect("Invalid wallet address");
        
        SubscribeRequest {
            accounts: HashMap::from([
                (target_wallet.to_string(), SubscribeRequestFilterAccounts {
                    account: vec![target_wallet.to_string()],
                    owner: vec![],
                    filters: vec![],
                    nonempty_txn_signature: Some(false),
                })
            ]),
            slots: HashMap::new(),
            transactions: HashMap::from([
                ("wallet_txs".to_string(), SubscribeRequestFilterTransactions {
                    vote: Some(false),
                    failed: Some(false),
                    signature: None,
                    account_include: vec![target_wallet.to_string(), self.fee_id.to_string()],
                    account_exclude: vec![],
                    account_required: vec![target_wallet.to_string()],
                })
            ]),
            blocks: HashMap::new(),
            blocks_meta: HashMap::new(),
            entry: HashMap::new(),
            transactions_status: HashMap::new(),
            commitment: Some(CommitmentLevel::Processed as i32),
            accounts_data_slice: vec![],
            ping: None,
            from_slot: Some(0),
        }
    }

    async fn monitor_wallet_stream(&self, stream: &mut (impl Stream<Item = Result<SubscribeUpdate, Status>> + Unpin)) {
        while let Some(message) = stream.next().await {
            match message {
                Ok(msg) => {
                    match msg.update_oneof {
                        Some(UpdateOneof::Transaction(tx)) => {
                            if let Some(meta) = tx.transaction {
                                info!("TRANSACTION META: {:?}", meta);
                                if let Err(e) = self.process_wallet_transaction(meta).await {
                                    error!("Failed to process wallet transaction: {}", e);
                                }
                            }
                        }
                        Some(UpdateOneof::Ping(_)) => {
                            // Handle ping message if needed
                            continue;
                        }
                        _ => {
                            // Handle other message types if needed
                            continue;
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

    async fn process_wallet_transaction(&self, meta: yellowstone_grpc_proto::prelude::SubscribeUpdateTransactionInfo) -> Result<(), Box<dyn Error + Send + Sync>> {
        let message_clone = &meta.transaction.as_ref().unwrap().message.as_ref().unwrap().clone();

        if let Some(transaction) = &meta.transaction {
            if let Some(message) = &transaction.message {
                // Process instructions
                for instruction in &message.instructions {
                    info!("Instruction: {:?}", instruction);
                    if instruction.data.len() >= 8 {
                        let type_id = &instruction.data[0..8];
                        
                        // Check each type independently instead of using if-else
                        if type_id == WALLET_BUY_DISCRIMINATOR.as_slice() {
                            info!("Detected wallet buy transaction");
                            if let Some(meta_info) = &meta.meta {
                                

                                let mut new_coin = match self.handle_wallet_buy_instruction(&meta_info, &message_clone, instruction, &meta).await {
                                    Ok(coin) => coin,
                                    Err(e) => {
                                        error!("Failed to extract coin details from transaction {:?}: {}", &message_clone, e);
                                        return Err(e);
                                    }
                                };
                                info!("Successfully extracted coin details from transaction A.B.C {:?}", &new_coin.associated_bonding_curve);

                                let coin = Arc::new(Mutex::new(new_coin));
                                if let Err(e) = self.coins_to_buy_sender.send(coin).await {
                                    error!("Failed to send coin to buy channel: {}", e);
                                    return Err(Box::new(MonitorError::NoCreatorBuy));
                                }
                                
                                return Ok(());
                            }
                        }
                        
                        if type_id == WALLET_SELL_DISCRIMINATOR.as_slice() {
                            info!("Detected wallet sell transaction");
                            if let Some(meta_info) = &meta.meta {
                                //self.handle_wallet_sell_instruction(&meta_info, &message_clone, instruction).await?;
                            }
                        }
                    }
                }
            }
        }
        Err(Box::new(MonitorError::NoCreatorBuy))
    }

    async fn handle_wallet_buy_instruction(
        &self,
        meta_info: &yellowstone_grpc_proto::prelude::TransactionStatusMeta,
        message_clone: &yellowstone_grpc_proto::prelude::Message,
        instruction: &yellowstone_grpc_proto::prelude::CompiledInstruction,
        transaction: &SubscribeUpdateTransactionInfo
    ) -> Result<Coin, Box<dyn Error + Send + Sync>> {
        // Check inner instructions for buy instruction
        for inner_instructions in &meta_info.inner_instructions {
            for inner_ix in &inner_instructions.instructions {
                if inner_ix.data.len() >= 8 {
                    let inner_type_id = &inner_ix.data[0..8];
                    if inner_type_id == INSTRUCTION_BUY {
                        info!("Found inner buy instruction");
                        info!("Inner instruction accounts: {:?}", inner_ix.accounts);
                        
                        // Get the program ID from the inner instruction
                        let program_id = &message_clone.account_keys[inner_ix.program_id_index as usize];
                        info!("Program ID: {:?}", program_id);
                        
                        // Print all account keys

                        let solana_instruction = solana_sdk::instruction::CompiledInstruction {
                            program_id_index: inner_ix.program_id_index as u8,
                            accounts: inner_ix.accounts.clone(),
                            data: inner_ix.data.clone(),
                        };
                        
                        let accounts = BuyAccounts::decode_account_keys(&message_clone.account_keys)
                            .map_err(|e| Box::new(MonitorError::NoCreatorBuy))?;
                        info!("Accounts: {:?}", accounts);

                        info!("Successfully got buy accounts: {:?}", accounts);

                        let buy_data = BuyInstructionData::try_from_slice(&solana_instruction.data[8..])
                            .map_err(|e| {
                                error!("Failed to decode buy data: {}", e);
                                MonitorError::NoCreatorBuy
                            })?;
                        info!("Successfully decoded buy data: {:?}", buy_data);

                        let mut new_coin = Coin {
                            mint_address: Some(accounts.mint.pubkey),
                            token_bonding_curve: accounts.bonding_curve.pubkey,
                            associated_bonding_curve: accounts.associated_bonding_curve.pubkey,
                            event_authority: accounts.event_authority.pubkey,
                            creator: accounts.user.pubkey,
                            creator_purchased: true,
                            creator_ata: Some(accounts.associated_user.pubkey),
                            wallet_tracking: true,
                            ..Default::default()
                        };

                                        // Get creator buy information
                        match self.wallet_fetch_creator_buy(&mut new_coin, &transaction, &buy_data).await {
                            Ok(_) => {
                                info!("Successfully fetched mint details for signature {:?}", &transaction.signature);
                                return Ok(new_coin)
                            }
                            Err(e) => {
                                error!("Failed to fetch creator buy info for transaction {:?}: {}", &transaction.signature, e);
                                return Err(e)
                            }
                        }
                    }
                }
            }
        }
        Err(Box::new(MonitorError::NoCreatorBuy))
    }

    async fn handle_wallet_sell_instruction(
        &self,
        meta_info: &yellowstone_grpc_proto::prelude::TransactionStatusMeta,
        message_clone: &yellowstone_grpc_proto::prelude::Message,
        instruction: &yellowstone_grpc_proto::prelude::CompiledInstruction,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Check inner instructions for sell instruction
        for inner_instructions in &meta_info.inner_instructions {
            for inner_ix in &inner_instructions.instructions {
                if inner_ix.data.len() >= 8 {
                    let inner_type_id = &inner_ix.data[0..8];
                    if inner_type_id == INSTRUCTION_SELL {
                        info!("Found inner sell instruction");
                        info!("Inner instruction accounts: {:?}", inner_ix.accounts);
                        
                        let resolved_accounts: Vec<Pubkey> = message_clone.account_keys.iter().enumerate()
                        .map(|(i, key)| {
                            let pubkey = Pubkey::from_str(&bs58::encode(key).into_string()).unwrap();
                            info!("SELL Account {}: {}", i, pubkey);
                            pubkey
                        }).collect();

                        // Get the program ID from the inner instruction
                        let program_id = &message_clone.account_keys[inner_ix.program_id_index as usize];
                        info!("Program ID: {:?}", program_id);
                        
                        // Print all account keys
                        let accounts = SellAccounts::decode_account_keys(&message_clone.account_keys);


                        let solana_instruction = solana_sdk::instruction::CompiledInstruction {
                            program_id_index: inner_ix.program_id_index as u8,
                            accounts: inner_ix.accounts.clone(),
                            data: inner_ix.data.clone(),
                        };


                        info!("Successfully got sell accounts: {:?}", accounts);
                        let sell_data = SellInstructionData::try_from_slice(&solana_instruction.data[8..])
                            .map_err(|e| {
                                error!("Failed to decode sell data: {:?}", e);
                                MonitorError::NoCreatorSell
                            })?;
                        info!("Successfully decoded sell data: {:?}", sell_data);

                        // Get the mint address from the accounts
                        let mint_address = accounts.unwrap().mint.pubkey;
                        let mint_str = mint_address.to_string();
                        
                        // Check both main coins and pending coins
                        //let mut coins = self.coins.lock().await;
                        let mut pending_coins = self.pending_coins.lock().await;
                        
                        // First check pending coins since these are more recent
                        if let Some(coin_arc) = pending_coins.get(&mint_str) {
                            let mut coin = coin_arc.lock().await;
                            if coin.creator_purchased && !coin.creator_sold {
                                info!("Creator sold coin we were tracking (from pending): {}", mint_str);
                                coin.creator_sold = true;
                                /* 
                                if let Err(e) = self.coins_to_sell_sender.send(Arc::new(Mutex::new(coin.clone()))).await {
                                    error!("Failed to send pending coin to sell channel: {}", e);
                                }
                                */
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }


    /// Extracts creator buy information from a transaction
    async fn wallet_fetch_creator_buy(&self,coin: &mut Coin, transaction: &SubscribeUpdateTransactionInfo, buy_data: &BuyInstructionData) -> Result<(), Box<dyn Error + Send + Sync>> {
        let message_clone = &transaction.transaction.as_ref().unwrap().message.as_ref().unwrap().clone();

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
                    
                        let offset = 8; // Skip the first 8 bytes (discriminator)
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
                        // Process the instruction here
                    }
                }
            }

            if buy_data.max_sol_cost == 0 {
                error!("Invalid buy amount (0 SOL)");
                return Err(Box::new(MonitorError::NoCreatorBuy));
            }

            info!("Successfully decoded buy data: {} SOL", buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64);
            coin.creator_purchased = true;
            coin.creator_purchase_sol = 0.99 * buy_data.max_sol_cost as f64 / LAMPORTS_PER_SOL as f64;
            
            info!("Successfully updated coin with buy information");
            return Ok(());
        }

        error!("No valid buy instruction found in transaction");
        Err(Box::new(MonitorError::NoCreatorBuy))
    }


    async fn handle_event_instruction(
        &self,
        meta_info: &yellowstone_grpc_proto::prelude::TransactionStatusMeta,
        message_clone: &yellowstone_grpc_proto::prelude::Message,
        instruction: &yellowstone_grpc_proto::prelude::CompiledInstruction,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Add your event handling logic here
        info!("Processing event instruction");
        // TODO: Implement event handling logic
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yellowstone_grpc_proto::prelude::{Message, CompiledInstruction, TransactionStatusMeta, InnerInstructions};

    // Mock struct for testing
    struct MockBot {
        coins: Arc<Mutex<HashMap<String, Arc<Mutex<Coin>>>>>,
    }

    impl MockBot {
        fn new() -> Self {
            Self {
                coins: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        async fn handle_wallet_buy_instruction(
            &self,
            meta_info: &yellowstone_grpc_proto::prelude::TransactionStatusMeta,
            message_clone: &yellowstone_grpc_proto::prelude::Message,
            instruction: &yellowstone_grpc_proto::prelude::CompiledInstruction,
            transaction: &SubscribeUpdateTransactionInfo,
        ) -> Result<Coin, Box<dyn Error + Send + Sync>> {
            // Similar implementation as Bot but simplified for testing
            for inner_instructions in &meta_info.inner_instructions {
                for inner_ix in &inner_instructions.instructions {
                    if inner_ix.data.len() >= 8 && inner_ix.data[0..8] == INSTRUCTION_BUY {
                        let accounts = BuyAccounts::decode_account_keys(&message_clone.account_keys)
                            .map_err(|e| Box::new(MonitorError::NoCreatorBuy))?;
                        return Ok(Coin {
                            mint_address: Some(accounts.mint.pubkey),
                            token_bonding_curve: accounts.bonding_curve.pubkey,
                            associated_bonding_curve: accounts.associated_bonding_curve.pubkey,
                            event_authority: accounts.event_authority.pubkey,
                            creator: accounts.user.pubkey,
                            ..Default::default()
                        });
                    }
                }
            }
            Err(Box::new(MonitorError::NoCreatorBuy))
        }
    }

    fn create_mock_transaction_info(instruction_data: Vec<u8>, account_keys: Vec<Vec<u8>>, inner_instruction_data: Option<Vec<u8>>) -> SubscribeUpdateTransactionInfo {
        let instruction = CompiledInstruction {
            program_id_index: 0,
            accounts: vec![0, 1, 2],
            data: instruction_data,
        };

        let message = Message {
            account_keys,
            instructions: vec![instruction],
            ..Default::default()
        };

        let inner_instructions = if let Some(data) = inner_instruction_data {
            vec![InnerInstructions {
                index: 0,
                instructions: vec![yellowstone_grpc_proto::prelude::InnerInstruction {
                    program_id_index: 0,
                    accounts: vec![0, 1, 2],
                    data,
                    stack_height: Some(0),
                }],
            }]
        } else {
            vec![]
        };

        let meta = TransactionStatusMeta {
            inner_instructions,
            ..Default::default()
        };

        SubscribeUpdateTransactionInfo {
            transaction: Some(yellowstone_grpc_proto::prelude::Transaction {
                message: Some(message),
                ..Default::default()
            }),
            meta: Some(meta),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_handle_wallet_buy_instruction() {
        let bot = MockBot::new();
        
        let mock_tx = create_mock_transaction_info(
            WALLET_BUY_DISCRIMINATOR.to_vec(),
            vec![
                vec![1; 32],  // 0: user
                vec![2; 32],  // 1: associated_user
                vec![3; 32],  // 2: fee_recipient
                vec![4; 32],  // 3: bonding_curve
                vec![5; 32],  // 4: associated_bonding_curve
                vec![6; 32],  // 5: mint
                vec![7; 32],  // 6: system_program
                vec![8; 32],  // 7: token_program
                vec![9; 32],  // 8: rent
                vec![10; 32], // 9: event_authority
                vec![11; 32], // 10: program
                vec![12; 32], // 11: global
                vec![13; 32], // 12: event_authority
                vec![14; 32], // 13: program
            ],
            Some(INSTRUCTION_BUY.to_vec())
        );

        let result = bot.handle_wallet_buy_instruction(
            mock_tx.meta.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap().instructions[0],
            &mock_tx
        ).await;

        assert!(result.is_ok(), "Should handle wallet buy instruction successfully");
        if let Ok(coin) = result {
            assert!(coin.mint_address.is_some(), "Mint address should be set");
            assert_ne!(coin.token_bonding_curve, Pubkey::default(), "Token bonding curve should be set");
            assert_ne!(coin.associated_bonding_curve, Pubkey::default(), "Associated bonding curve should be set");
            assert_ne!(coin.event_authority, Pubkey::default(), "Event authority should be set");
            assert_ne!(coin.creator, Pubkey::default(), "Creator should be set");
        }
    }

    #[tokio::test]
    async fn test_handle_wallet_buy_instruction_invalid_data() {
        let bot = MockBot::new();
        
        let mock_tx = create_mock_transaction_info(
            vec![0; 8], // Invalid discriminator
            vec![
                vec![1; 32],  // 0: user
                vec![2; 32],  // 1: associated_user
                vec![3; 32],  // 2: fee_recipient
                vec![4; 32],  // 3: bonding_curve
                vec![5; 32],  // 4: associated_bonding_curve
                vec![6; 32],  // 5: mint
                vec![7; 32],  // 6: system_program
                vec![8; 32],  // 7: token_program
                vec![9; 32],  // 8: rent
                vec![10; 32], // 9: event_authority
                vec![11; 32], // 10: program
                vec![12; 32], // 11: global
                vec![13; 32], // 12: event_authority
                vec![14; 32], // 13: program
            ],
            None
        );

        let result = bot.handle_wallet_buy_instruction(
            mock_tx.meta.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap().instructions[0],
            &mock_tx
        ).await;

        assert!(result.is_err(), "Should fail with invalid instruction data");
    }

    #[tokio::test]
    async fn test_handle_wallet_buy_instruction_no_inner_instructions() {
        let bot = MockBot::new();
        
        let mock_tx = create_mock_transaction_info(
            WALLET_BUY_DISCRIMINATOR.to_vec(),
            vec![
                vec![1; 32],  // 0: user
                vec![2; 32],  // 1: associated_user
                vec![3; 32],  // 2: fee_recipient
                vec![4; 32],  // 3: bonding_curve
                vec![5; 32],  // 4: associated_bonding_curve
                vec![6; 32],  // 5: mint
                vec![7; 32],  // 6: system_program
                vec![8; 32],  // 7: token_program
                vec![9; 32],  // 8: rent
                vec![10; 32], // 9: event_authority
                vec![11; 32], // 10: program
                vec![12; 32], // 11: global
                vec![13; 32], // 12: event_authority
                vec![14; 32], // 13: program
            ],
            None // No inner instructions
        );

        let result = bot.handle_wallet_buy_instruction(
            mock_tx.meta.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap().instructions[0],
            &mock_tx
        ).await;

        assert!(result.is_err(), "Should fail when no inner instructions present");
    }

    #[tokio::test]
    async fn test_handle_wallet_buy_instruction_wrong_inner_instruction() {
        let bot = MockBot::new();
        
        let mock_tx = create_mock_transaction_info(
            WALLET_BUY_DISCRIMINATOR.to_vec(),
            vec![vec![1; 32], vec![2; 32], vec![3; 32]],
            Some(vec![0; 8]) // Wrong inner instruction data
        );

        let result = bot.handle_wallet_buy_instruction(
            mock_tx.meta.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap(),
            &mock_tx.transaction.as_ref().unwrap().message.as_ref().unwrap().instructions[0],
            &mock_tx
        ).await;

        assert!(result.is_err(), "Should fail with wrong inner instruction data");
    }
} 