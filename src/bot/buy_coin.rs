use {
    crate::{
        bot::{bonding_curve::BondingCurveData, constants::COMPUTE_UNIT_LIMIT, constants::INSTRUCTION_BUY, structs::{Bot, Coin}},
        bot::constants::{MINT_AUTH_ID},
        pump::buy::{BuyAccounts, BuyInstructionData},
    }, borsh::BorshSerialize, log::info, solana_program::{
        instruction::Instruction, pubkey::Pubkey, system_program
    }, solana_sdk::{
        compute_budget::ComputeBudgetInstruction, signer::Signer, transaction::Transaction
    },
     spl_associated_token_account::{
        get_associated_token_address, instruction as spl_ata
    }, spl_token, std::sync::Arc, tokio::sync::Mutex,
    solana_program::instruction::AccountMeta,
    solana_sdk::commitment_config::CommitmentLevel, 
    solana_client::rpc_config::RpcSendTransactionConfig,
    tokio::time::sleep,
    std::time::Duration,
    solana_sdk::commitment_config::CommitmentConfig
};

impl Bot {
    pub async fn buy_coin(&self, coin: Arc<Mutex<Coin>>, should_geyser: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        /* 
        if !self.jito_manager.is_jito_leader() {
            info!("Jito is not leader, skipping buy");
            return Err("Jito is not leader, skipping buy".into());
        }
        */
        let mut coin_guard = coin.lock().await;
        // Set exited_buy_coin to true at the beginning (equivalent to Go's defer)
        coin_guard.exited_buy_coin = true;
        if coin_guard.mint_address.is_none() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Nil Coin")));
        }

        // Display buy status
        let buy_status = format!(
            "Attempting to buy {} ({:?})",
            coin_guard.mint_address.unwrap(),
            coin_guard.pickup_time.elapsed()
        );
        self.status(&buy_status);

        // Calculate ATA address
        
        let mut ata_address = Pubkey::default();
        let should_create_ata = if self.skip_ata_lookup {
            true
        }
        else {
            coin_guard.status(&format!("Checking associated token: {}", ata_address));
            ata_address = self.calculate_ata_address(&coin_guard).await?;
            self.should_create_ata(&ata_address).await?
        };
        

        // Fetch bonding curve data
        coin_guard.status("Fetching bonding curve");
        let bcd: BondingCurveData;
        if should_geyser{
            info!("Using geyser bonding curve");
            bcd = coin_guard.bonding_curve_data.as_ref().unwrap().clone();
        } else {
            bcd = BondingCurveData::fetchBondingCurve(&self.rpc_client, &coin_guard.token_bonding_curve).await?;
        }
        
        coin_guard.status(&format!("Fetched bonding curve, ({})", bcd));

        // Check if we're late to buy
        if coin_guard.late_to_buy(&bcd)  && !coin_guard.wallet_tracking {
            return Err(Box::<dyn std::error::Error + Send + Sync>::from("Coin has multiple buyers (BCD)"));
        }

        // Set buy price and calculate tokens to buy
        coin_guard.buy_price = self.buy_amount_lamports;
        info!("BUY PRICE: {:?}", self.buy_amount_lamports);
        let tokens_to_buy = bcd.calculate_buy_quote(self.buy_amount_lamports, 0.25);
        
        coin_guard.status(&format!("Calculated buy quote: {} tokens for {} SOL",tokens_to_buy, coin_guard.buy_price));

        // Create instructions
        let mut instructions = vec![];

        // Add compute budget instructions
        instructions.push(
            ComputeBudgetInstruction::set_compute_unit_limit(
                COMPUTE_UNIT_LIMIT,
            )
        );

        //PRIORITY FEE

        instructions.push(
            ComputeBudgetInstruction::set_compute_unit_price(
                self.fee_micro_lamports
            )
        );


        // Add ATA creation if needed

        if should_create_ata {
            let (ata, create_ata_instruction) = self.create_ata(&coin_guard).await?;
            instructions.push(create_ata_instruction);
            ata_address = ata;
        }

        // Create buy instruction
        let buy_instruction = self.create_buy_instruction(tokens_to_buy as u64, &coin_guard, ata_address);
        instructions.push(buy_instruction);

        // Handle Jito if enabled
        let enable_jito = self.jito_manager.is_jito_leader();
        
        if enable_jito {
            coin_guard.status("Jito leader, setting tip & removing priority fee inst");
            let tip_inst = self.jito_manager.generate_tip_instruction().await?;
            instructions.push(tip_inst);
            // Remove priority fee when using Jito tip
            instructions.remove(1);
        }
        
        // Create and send transaction
        coin_guard.status("Creating transaction");
        let transaction = self.create_transaction(instructions).await?;
        // Stop mint monitoring after sending transaction
        *self.is_monitoring.lock().await = false;
        info!("Stopped mint monitoring after sending buy transaction");

        coin_guard.status("Sending transaction");
        let signature = self.sign_and_send_transaction(&transaction, enable_jito).await?;

        // Wait for 2 seconds to ensure transaction is confirmed

        // Update coin state after waiting
        coin_guard.bot_purchased = true;
        coin_guard.tokens_held = tokens_to_buy as u64;
        coin_guard.associated_token_account = Some(ata_address);
        coin_guard.buy_transaction_signature = Some(signature);
        coin_guard.creator_sold = true;
        coin_guard.status("Buy transaction completed");

        Ok(())
    }

    async fn calculate_ata_address(&self, coin: &Coin) -> Result<Pubkey, Box<dyn std::error::Error + Send + Sync>> {
        Ok(get_associated_token_address(
            &self.private_key.pubkey(),
            &coin.mint_address.unwrap()
        ))
    }

    async fn should_create_ata(&self, ata_address: &Pubkey) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.rpc_client.get_account(ata_address).await.is_err())
    }

    async fn create_ata(&self, coin: &Coin) -> Result<(Pubkey, Instruction), Box<dyn std::error::Error + Send + Sync>> {
        let ata = get_associated_token_address(
            &self.private_key.pubkey(),
            &coin.mint_address.unwrap()
        );

        let instruction = spl_ata::create_associated_token_account(
            &self.private_key.pubkey(),
            &self.private_key.pubkey(),
            &coin.mint_address.unwrap(),
            &spl_token::id(),
        );

        Ok((ata, instruction))
    }

    fn create_buy_instruction(&self, tokens_to_buy: u64, coin: &Coin, ata: Pubkey) -> Instruction {
        let accounts = BuyAccounts::new(
            self.global_address,
            self.fee_id,
            coin.mint_address.unwrap(),
            coin.token_bonding_curve,
            coin.associated_bonding_curve,
            ata,
            self.private_key.pubkey(),
            system_program::id(),
            spl_token::id(),
            solana_sdk::sysvar::rent::id(),
            coin.event_authority,
            self.program_id,
        );

        let data: BuyInstructionData = BuyInstructionData {
            amount: tokens_to_buy,
            max_sol_cost: self.buy_amount_lamports,
        };

        // Add buy discriminator
        let mut instruction_data = INSTRUCTION_BUY.to_vec();
        instruction_data.extend_from_slice(&data.try_to_vec().unwrap());

        //info!("CHECK ACCOUNTS: {:?}", accounts.to_account_metas());
        Instruction {
            program_id: self.program_id,
            accounts: accounts.to_account_metas(),
            data: instruction_data,
        }
    }

    pub async fn create_transaction(&self, instructions: Vec<Instruction>) -> Result<Transaction, Box<dyn std::error::Error + Send + Sync>> {
        let blockhash_guard = self.blockhash.lock().await;
        info!("Current blockhash state: {:?}", blockhash_guard);
        let blockhash = blockhash_guard.ok_or("No blockhash available")?;
        info!("Using blockhash: {:?}", blockhash);
        
        Ok(Transaction::new_signed_with_payer(
            &instructions,
            Some(&self.private_key.pubkey()),
            &vec![&*self.private_key],
            blockhash
        ))
    }

    pub async fn sign_and_send_transaction(&self, transaction: &Transaction, enable_jito: bool) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let signature = if enable_jito {
            self.jito_manager.send_transaction(transaction).await?
        } else {
            let config = RpcSendTransactionConfig {
                skip_preflight: true,
                max_retries: Some(0),
                .. RpcSendTransactionConfig::default()
            };
            self.rpc_client.send_and_confirm_transaction_with_spinner_and_config(
                transaction,
                CommitmentConfig::confirmed(),
                config,
            ).await?.to_string()
        };

        Ok(signature)
    }
}