
use {
    crate::{
        bot::{Bot, structs::Coin, bonding_curve::BondingCurveData},
    },
    lazy_static::lazy_static,
    solana_client::{
        nonblocking::rpc_client::RpcClient,
        rpc_config::{RpcTransactionConfig, RpcSignatureSubscribeConfig, RpcSendTransactionConfig},
        rpc_client::GetConfirmedSignaturesForAddress2Config,
        rpc_response::RpcSignatureResult,
    },
    solana_sdk::{
        signature::Signature,
        transaction::Transaction,
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
    },
    solana_transaction_status::EncodedTransaction,
    std::{
        collections::HashSet,
        str::FromStr,
        time::Duration,
    },
    tokio::time::sleep,
    log::{info, error},
    anyhow::{Result, anyhow},
    bincode,
    base64,
    futures::StreamExt,
    solana_transaction_status::UiTransactionEncoding,
    solana_sdk::bs58,
};

lazy_static! {
    static ref EXCHANGE_ADDRESSES: HashSet<&'static str> = {
        let mut set = HashSet::new();
        set.insert("AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2");
        set.insert("42brAgAVNzMBP7aaktPvAmBSPEkehnFQejiZc53EpJFd");
        set.insert("ASTyfSima4LLAdDgoFGkgqoKowG1LZFDr9fAQrg7iaJZ");
        set.insert("H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS");
        set.insert("GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE");
        set.insert("5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9");
        set.insert("2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S");
        set.insert("5VCwKtCXgCJ6kit5FybXjvriW3xELsFDhYrPSqtJNmcD");
        set.insert("2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm");
        set
    };
}

pub fn is_exchange_address(address: &str) -> bool {
    EXCHANGE_ADDRESSES.contains(address)
}

impl Bot {
    /// Signs and sends a transaction, optionally using Jito
    pub async fn sign_and_send_tx(&self, tx: &Transaction, enable_jito: bool) -> Result<String> {
        let signature = tx.signatures[0];
        let start_ts = std::time::Instant::now();

        if enable_jito {
            info!("Sending transaction (Jito) {}", signature);

            self.jito_manager.broadcast_bundle(&[tx.clone()]).await?;

            self.wait_for_transaction_complete(&signature).await?;

            let latency = start_ts.elapsed().as_millis();
            info!("Sent transaction (Jito) {} with latency {} ms", signature, latency);

            return Ok(signature.to_string());
        }

        self.send_tx_vanilla(tx).await
    }

    /// Sends a transaction through vanilla (non-Jito) RPCs
    async fn send_tx_vanilla(&self, tx: &Transaction) -> Result<String> {
        let signature = tx.signatures[0];
        info!("Sending Vanilla TX to Dedicated & Free RPCs: {}", signature);

        // Send to dedicated RPC
        let tx_clone = tx.clone();
        let rpc_client = self.rpc_client.clone();
        tokio::spawn(async move {
            if let Err(e) = rpc_client.send_transaction_with_config(
                &tx_clone,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..Default::default()
                },
            ).await {
                error!("Error Sending Vanilla TX (Dedicated RPC): {}", e);
            }
        });

        // Send to alternate RPCs
        for rpc_client in &self.send_tx_clients {
            let tx_clone = tx.clone();
            let rpc_client = rpc_client.clone();
            tokio::spawn(async move {
                if let Err(e) = Self::send_one_vanilla_tx(&tx_clone, &rpc_client).await {
                    if e.to_string().contains("429") {
                        error!("Error Sending 1 Vanilla TX (Free RPC) (Ratelimited)");
                    } else {
                        error!("Error Sending 1 Vanilla TX (Free RPC): {}", e);
                    }
                }
            });
        }

        self.wait_for_transaction_complete(&signature).await?;
        Ok(signature.to_string())
    }

    /// Sends a single vanilla transaction through one RPC
    async fn send_one_vanilla_tx(tx: &Transaction, rpc_client: &RpcClient) -> Result<()> {
        rpc_client.send_transaction_with_config(
            tx,
            RpcSendTransactionConfig {
                skip_preflight: true,
                ..Default::default()
            },
        ).await?;
        Ok(())
    }

    /// Waits for a transaction to complete with a timeout
    async fn wait_for_transaction_complete(&self, signature: &Signature) -> Result<()> {
        info!("Waiting for transaction {} to complete", signature);

        let (mut subscription, _) = self.ws_client.signature_subscribe(
            signature,
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                ..Default::default()
            }),
        ).await?;

        tokio::select! {
            result = subscription.next() => {
                match result {
                    Some(response) => {
                        match response.value {
                            RpcSignatureResult::ProcessedSignature(result) => {
                                if let Some(err) = result.err {
                                    return Err(anyhow!("Transaction error: {:?}", err));
                                }
                            }
                            RpcSignatureResult::ReceivedSignature(_) => {}
                        }
                        Ok(())
                    }
                    None => Err(anyhow!("Subscription ended unexpectedly"))
                }
            }
            _ = sleep(Duration::from_secs(120)) => {
                Err(anyhow!("Transaction timeout"))
            }
        }
    }

    /// Fetches the N most recent transactions for an address
    pub async fn fetch_n_last_trans(&self, number_sigs: usize, address: &str) -> Result<Vec<Transaction>> {
        info!("Fetching {} recent transactions for address: {}", number_sigs, address);
        
        let pubkey = Pubkey::from_str(address)
            .map_err(|e| anyhow!("Invalid address: {}", e))?;

        // Get signatures
        let signatures = self.rpc_client.get_signatures_for_address_with_config(
            &pubkey,
            GetConfirmedSignaturesForAddress2Config {
                limit: Some(number_sigs),
                commitment: Some(CommitmentConfig::confirmed()),
                before: None,
                until: None,
                ..Default::default()
            },
        ).await.map_err(|e| {
            if e.to_string().contains("context deadline") {
                error!("Context timeout for {}", address);
                anyhow!("Context timeout")
            } else {
                error!("Failed to fetch transactions for {}: {}", address, e);
                anyhow!("Failed to fetch transactions: {}", e)
            }
        })?;

        // Get transactions in parallel
        let mut transactions = Vec::with_capacity(signatures.len());
        let mut handles = Vec::with_capacity(signatures.len());

        for sig in signatures {
            let rpc_client = self.rpc_client.clone();
            let handle = tokio::spawn(async move {
                match Signature::from_str(&sig.signature) {
                    Ok(signature) => {
                        rpc_client.get_transaction_with_config(
                            &signature,
                            RpcTransactionConfig {
                                encoding: Some(UiTransactionEncoding::Json),
                                commitment: Some(CommitmentConfig::confirmed()),
                                max_supported_transaction_version: Some(0),
                            },
                        ).await
                    }
                    Err(e) => {
                        error!("Failed to parse signature: {}", e);
                        Err(solana_client::client_error::ClientError::new_with_request(
                            solana_client::client_error::ClientErrorKind::Custom(format!("Invalid signature: {}", e)),
                            solana_client::rpc_request::RpcRequest::GetTransaction,
                        ))
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            match handle.await {
                Ok(Ok(tx)) => {
                    match tx.transaction.transaction {
                        EncodedTransaction::Json(tx_json) => {
                            match &tx_json.message {
                                solana_transaction_status::UiMessage::Raw(message) => {
                                    // Convert JSON transaction to legacy transaction
                                    let fee_payer = Pubkey::from_str(&message.account_keys[0])
                                        .map_err(|e| {
                                            error!("Failed to parse fee payer: {}", e);
                                            e
                                        })?;
                                    
                                    // Create message first
                                    let mut legacy_message = solana_sdk::message::Message::new(
                                        &[],
                                        Some(&fee_payer)
                                    );

                                    // Set account keys
                                    legacy_message.account_keys = message.account_keys.iter()
                                        .map(|key| Pubkey::from_str(key))
                                        .collect::<Result<Vec<_>, _>>()
                                        .map_err(|e| {
                                            error!("Failed to parse account keys: {}", e);
                                            anyhow!("Failed to parse account keys: {}", e)
                                        })?;

                                    // Set recent blockhash
                                    legacy_message.recent_blockhash = message.recent_blockhash.parse()
                                        .map_err(|e| {
                                            error!("Failed to parse blockhash: {}", e);
                                            anyhow!("Failed to parse blockhash: {}", e)
                                        })?;

                                    // Set instructions
                                    legacy_message.instructions = message.instructions.iter()
                                        .map(|ix| {
                                            solana_sdk::instruction::CompiledInstruction {
                                                program_id_index: ix.program_id_index,
                                                accounts: ix.accounts.clone(),
                                                data: bs58::decode(&ix.data).into_vec().unwrap_or_default(),
                                            }
                                        })
                                        .collect();

                                    info!("Successfully converted JSON transaction to legacy format");
                                    transactions.push(Transaction {
                                        signatures: vec![Signature::from_str(&tx_json.signatures[0]).unwrap()],
                                        message: legacy_message,
                                    });
                                }
                                _ => {
                                    error!("Unsupported message format");
                                }
                            }
                        }
                        EncodedTransaction::Binary(data, _) => {
                            if let Ok(decoded) = bincode::deserialize::<Transaction>(&base64::decode(&data).unwrap_or_default()) {
                                transactions.push(decoded);
                            }
                        }
                        EncodedTransaction::LegacyBinary(data) => {
                            if let Ok(decoded) = bincode::deserialize::<Transaction>(&base64::decode(&data).unwrap_or_default()) {
                                transactions.push(decoded);
                            }
                        }
                        EncodedTransaction::Accounts(_) => {
                            error!("Accounts encoded transaction not supported");
                        }
                    }
                }
                Ok(Err(e)) => error!("Failed to get transaction: {}", e),
                Err(e) => error!("Task join error: {}", e),
            }
        }

        info!("Successfully fetched {} transactions", transactions.len());
        Ok(transactions)
    }
}
impl Coin {
    /// Checks if it's too late to buy based on virtual SOL reserves
    pub fn late_to_buy(&self, bcd: &BondingCurveData) -> bool {
        let reserves_sol = bcd.virtual_sol_reserves as f64 / 1_000_000_000.0; // LAMPORTS_PER_SOL
        let reserves_less_creator_sol = reserves_sol - self.creator_purchase_sol as f64;

        // Consider data stale if someone is in with more than 0.1
        // NOTE: we deduct 30 solana since that's already in bonding curve, provided by pump.fun
        reserves_less_creator_sol - 30.0 > 0.1
    }
}

