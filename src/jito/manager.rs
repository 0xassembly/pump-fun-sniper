use {
    crate::jito::types::{ValidatorApiResponse, TipStreamInfo, ValidatorInfo},
    anyhow::{Result, anyhow},
    jito_sdk_rust::JitoJsonRpcSDK,
    reqwest::Client,
    solana_client::rpc_client::RpcClient,
    solana_program::instruction::Instruction,
    solana_sdk::{
        bs58,
        commitment_config::CommitmentConfig,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    std::{
        collections::HashMap,
        sync::{Arc, Mutex},
        time::Duration,
        str::FromStr,
    },
    tokio::{
        sync::broadcast,
        time::sleep,
    },
    log::{info, error},
};

const DEFAULT_TIP_AMOUNT: u64 = 1_000_000;

pub struct JitoManager {
    http_client: Client,
    rpc_client: Arc<RpcClient>,
    keypair: Arc<Keypair>,
    
    slot_index: Arc<Mutex<u64>>,
    epoch: Arc<Mutex<u64>>,
    
    validators: Arc<Mutex<HashMap<Pubkey, ValidatorInfo>>>,
    slot_leader: Arc<Mutex<HashMap<u64, Pubkey>>>,
    
    tip_info: Arc<Mutex<Option<TipStreamInfo>>>,
    jito_sdk: Arc<JitoJsonRpcSDK>,
    jito_validator_api: Arc<Option<String>>,
    jito_validator_endpoint:  Arc<Option<String>>
}

impl JitoManager {
    pub async fn new(rpc_client: Arc<RpcClient>, keypair: Arc<Keypair>, jito_validator_api: Arc<Option<String>>, jito_validator_endpoint: Arc<Option<String>>) -> Result<Self> {
        let jito_sdk = JitoJsonRpcSDK::new(jito_validator_api.as_deref().unwrap_or_default(), None);
        info!("jito_validator_api: {:?}", jito_validator_api);
        info!("jito_validator_endpoint: {:?}", jito_validator_endpoint);
        Ok(Self {
            http_client: Client::new(),
            rpc_client,
            keypair,
            slot_index: Arc::new(Mutex::new(0)),
            epoch: Arc::new(Mutex::new(0)),
            validators: Arc::new(Mutex::new(HashMap::new())),
            slot_leader: Arc::new(Mutex::new(HashMap::new())),
            tip_info: Arc::new(Mutex::new(None)),
            jito_sdk: Arc::new(jito_sdk),
            jito_validator_api: jito_validator_api,
            jito_validator_endpoint: jito_validator_endpoint
        })
    }

    pub async fn generate_tip_instruction(&self) -> Result<Instruction> {
        // Fixed tip amount for buys: 0.00256 SOL
        let tip_amount = 1_000;
        info!("Generating tip instruction for {:.5} SOL", tip_amount as f64 / 1e9);
        
        // Get a random tip account
        let tip_account = Pubkey::from_str(&self.jito_sdk.get_random_tip_account().await?)?;
        
        // Create a transfer instruction to the tip account
        let instruction = solana_sdk::system_instruction::transfer(
            &self.keypair.pubkey(),
            &tip_account,
            tip_amount,
        );
        
        Ok(instruction)
    }

    pub async fn generate_default_tip_instruction(&self) -> Result<Instruction> {
        // Default tip amount for sells: 0.00001 SOL
        let tip_amount = 1_000;
        info!("Generating default tip instruction for {:.5} SOL", tip_amount as f64 / 1e9);
        
        // Get a random tip account
        let tip_account = Pubkey::from_str(&self.jito_sdk.get_random_tip_account().await?)?;
        
        // Create a transfer instruction to the tip account
        let instruction = solana_sdk::system_instruction::transfer(
            &self.keypair.pubkey(),
            &tip_account,
            tip_amount,
        );
        
        Ok(instruction)
    }

    pub async fn broadcast_bundle(&self, transactions: &[Transaction]) -> Result<()> {
        // Convert transactions to base64 strings and create JSON array
        let tx_strings: Vec<String> = transactions.iter()
            .map(|tx| base64::encode(bincode::serialize(tx).unwrap()))
            .collect();
        
        let json_array = serde_json::Value::Array(
            tx_strings.iter()
                .map(|s| serde_json::Value::String(s.clone()))
                .collect()
        );

        // Send bundle using the SDK
        self.jito_sdk.send_bundle(Some(json_array), None).await?;
        Ok(())
    }

    fn generate_tip_amount(&self) -> u64 {
        self.tip_info
            .lock()
            .unwrap()
            .as_ref()
            .map(|info| (info.landed_tips_75th_percentile * 1e9) as u64)
            .unwrap_or(DEFAULT_TIP_AMOUNT)
    }

    pub async fn start(&self) -> Result<()> {
        // Sequential initialization
        self.fetch_tip_info().await?;
        info!("Tip info fetched successfully");
        
        self.fetch_jito_validators().await?;
        info!("Validators fetched successfully");
        
        self.fetch_leader_schedule().await?;
        info!("Leader schedule fetched successfully");
        
        self.fetch_epoch_info().await?;
        info!("Epoch info fetched successfully");

        // Only spawn epoch info task as it's the most critical
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = this.fetch_epoch_info().await {
                    error!("Failed to fetch epoch info: {}", e);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        info!("Jito manager started successfully");
        Ok(())
    }

    pub fn is_jito_leader(&self) -> bool {
        let slot_index = *self.slot_index.lock().unwrap();
        let slot_leader_guard = self.slot_leader.lock().unwrap();
        let validator_pubkey = match slot_leader_guard.get(&slot_index) {
            Some(v) => v,
            None => return false,
        };

        info!("Checking if validator is a Jito leader: {}", validator_pubkey);
        
        let validators_guard = self.validators.lock().unwrap();
        validators_guard
            .get(validator_pubkey)
            .map(|info| info.is_jito)
            .unwrap_or(false)
    }

    async fn fetch_leader_schedule(&self) -> Result<()> {
        info!("Fetching leader schedule");

        let schedule = self.rpc_client.get_leader_schedule(None)?;
        
        if let Some(schedule) = schedule {
            let mut slot_leader = HashMap::new();
            for (validator, slots) in schedule {
                let validator_pubkey = Pubkey::from_str(&validator)?;
                for slot in slots {
                    slot_leader.insert(slot as u64, validator_pubkey);
                }
            }
            *self.slot_leader.lock().unwrap() = slot_leader;
        }

        Ok(())
    }

    async fn fetch_epoch_info(&self) -> Result<()> {
        let info = self.rpc_client.get_epoch_info_with_commitment(CommitmentConfig::finalized())?;
        
        // Check if epoch changed first
        let epoch_changed = {
            let mut epoch = self.epoch.lock().unwrap();
            if *epoch != info.epoch {
                *epoch = info.epoch;
                true
            } else {
                false
            }
        };

        // Only update slot index if epoch hasn't changed
        if !epoch_changed {
            let mut slot_index = self.slot_index.lock().unwrap();
            *slot_index = info.slot_index;
            return Ok(());
        }

        // If epoch changed, update leader schedule and validators
        info!("New epoch detected ({}), updating schedules", info.epoch);
        self.fetch_leader_schedule().await?;
        self.fetch_jito_validators().await?;
        
        // Update slot index after schedule updates
        let mut slot_index = self.slot_index.lock().unwrap();
        *slot_index = info.slot_index;

        Ok(())
    }


    async fn fetch_jito_validators(&self) -> Result<()> {
        info!("Fetching jito-enabled validators");

        let response = self.http_client
            .get(self.jito_validator_endpoint.as_deref().unwrap_or_default())
            .header("accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch validators: {}", response.status()));
        }

        let api_response: ValidatorApiResponse = response.json().await?;
        
        // Get current vote accounts for mapping
        let vote_accounts = self.rpc_client.get_vote_accounts()?;
        let mut validators = HashMap::new();
        
        for account in vote_accounts.current {
            let node_pubkey = Pubkey::from_str(&account.node_pubkey)?;
            let vote_pubkey = Pubkey::from_str(&account.vote_pubkey)?;
            
            let is_jito = api_response.validators.iter()
                .any(|v| v.vote_account == account.vote_pubkey && v.running_jito);
            
            validators.insert(node_pubkey, ValidatorInfo {
                node_pubkey,
                vote_pubkey,
                is_jito,
            });
        }
        
        *self.validators.lock().unwrap() = validators;
        Ok(())
    }

    async fn fetch_tip_info(&self) -> Result<()> {
        // Get tip accounts from Jito SDK
        let tip_accounts = self.jito_sdk.get_tip_accounts().await?;
        
        // Calculate tip statistics from the accounts
        let tip_info = if let Ok(tip_data) = serde_json::from_value::<Vec<f64>>(tip_accounts) {
            let mut sorted_tips = tip_data;
            sorted_tips.sort_by(|a, b| a.partial_cmp(b).unwrap());
            
            let len = sorted_tips.len();
            if len > 0 {
                TipStreamInfo {
                    landed_tips_75th_percentile: sorted_tips[(len as f64 * 0.75) as usize],
                    landed_tips_95th_percentile: sorted_tips[(len as f64 * 0.95) as usize],
                    landed_tips_99th_percentile: sorted_tips[(len as f64 * 0.99) as usize],
                }
            } else {
                TipStreamInfo::default()
            }
        } else {
            TipStreamInfo::default()
        };
        
        info!(
            "Using tip values (75th={:.3}, 95th={:.3}, 99th={:.3} SOL)",
            tip_info.landed_tips_75th_percentile,
            tip_info.landed_tips_95th_percentile,
            tip_info.landed_tips_99th_percentile
        );
        
        // Update tip info
        *self.tip_info.lock().unwrap() = Some(tip_info);
        
        Ok(())
    }

    pub async fn send_transaction(&self, transaction: &Transaction) -> Result<String> {
        // Convert transaction to base58 string
        let tx_string = bs58::encode(bincode::serialize(transaction).unwrap()).into_string();
        let json_array = serde_json::Value::Array(vec![serde_json::Value::String(tx_string)]);

        // Send bundle using the SDK
        self.jito_sdk.send_bundle(Some(json_array), None).await?;
        Ok(transaction.signatures[0].to_string())
    }

    pub async fn send_transaction_bytes(&self, transaction_bytes: &[u8]) -> Result<String> {
        // Convert transaction bytes to base58 string
        let base58_tx = bs58::encode(transaction_bytes).into_string();
        let json_array = serde_json::Value::Array(vec![serde_json::Value::String(base58_tx)]);

        // Send bundle using the SDK
        self.jito_sdk.send_bundle(Some(json_array), None).await?;

        // Deserialize the transaction to get the signature
        let transaction: Transaction = bincode::deserialize(transaction_bytes)?;
        Ok(transaction.signatures[0].to_string())
    }
}

impl Clone for JitoManager {
    fn clone(&self) -> Self {
        Self {
            http_client: self.http_client.clone(),
            rpc_client: self.rpc_client.clone(),
            keypair: self.keypair.clone(),
            slot_index: self.slot_index.clone(),
            epoch: self.epoch.clone(),
            validators: self.validators.clone(),
            slot_leader: self.slot_leader.clone(),
            tip_info: self.tip_info.clone(),
            jito_sdk: self.jito_sdk.clone(),
            jito_validator_api: self.jito_validator_api.clone(),
            jito_validator_endpoint: self.jito_validator_endpoint.clone()
        }
    }
}
