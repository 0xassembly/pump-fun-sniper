use {
    
    crate::{
        bot::{bonding_curve::BondingCurveData, constants::{PROGRAM_ID, GLOBAL_ID, EVENT_AUTH_ID}}, jito::manager::JitoManager}, 
        
        anyhow::Error, clap::builder::Str, lazy_static::lazy_static, log::{error, info, warn}, 
        reqwest::{Client, Proxy}, 
        solana_client::{
        nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient as NonblockingRpcClient}, rpc_client::RpcClientConfig, rpc_config::{RpcContextConfig, RpcSendTransactionConfig}
    }, solana_sdk::{
        commitment_config::CommitmentConfig, hash::Hash, pubkey::Pubkey, signature::Keypair
    }, sqlx::MySqlPool, std::{
        collections::HashMap, default, fmt, sync::Arc, time::{Duration, Instant}
    }, thiserror::Error, tokio::sync::{mpsc, Mutex, Semaphore}, tonic::{
        codec::{CompressionEncoding, Streaming}, metadata::{errors::InvalidMetadataValue, AsciiMetadataValue, MetadataValue}, service::interceptor::InterceptedService, transport::channel::{Channel, ClientTlsConfig, Endpoint}, Request, Response, Status
    }, yellowstone_grpc_client::{
        GeyserGrpcBuilder, GeyserGrpcBuilderError, GeyserGrpcClient, GeyserGrpcClientError, Interceptor, InterceptorXToken
    }, yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, CommitmentLevel, SubscribeRequest,
        SubscribeRequestFilterSlots, SubscribeRequestPing, SubscribeUpdatePong,
        SubscribeUpdateSlot,
    },

};


#[derive(Error, Debug)]
pub enum BotError {
    #[error("MySQL DB Connection Error: {0}")]
    DbConnectionError(String),
    #[error("RPC Error: {0}")]
    RpcError(String),
    #[error("WebSocket Error: {0}")]
    WsError(String),
    #[error("gRPC Error: {0}")]
    GrpcError(String),
    #[error("Other Error: {0}")]
    Other(String),
}


impl From<anyhow::Error> for BotError {
    fn from(err: anyhow::Error) -> Self {
        BotError::Other(err.to_string())
    }
}

impl From<yellowstone_grpc_client::GeyserGrpcBuilderError> for BotError {
    fn from(err: yellowstone_grpc_client::GeyserGrpcBuilderError) -> Self {
        BotError::GrpcError(err.to_string())
    }
}

pub type GeyserGrpcClientResult<T> = Result<T, GeyserGrpcClientError>;

/// Main bot structure that handles buying and selling coins
pub struct Bot{
    pub rpc_client: Arc<NonblockingRpcClient>,
    pub ws_client: Arc<PubsubClient>,
    pub send_tx_clients: Vec<Arc<NonblockingRpcClient>>,
    pub private_key: Arc<Keypair>,
    pub db_connection: MySqlPool,
    
    pub fee_micro_lamports: u64,
    pub buy_amount_lamports: u64,

    pub pending_coins: Arc<Mutex<HashMap<String, Arc<Mutex<Coin>>>>>,
    pub coins_to_buy_sender: mpsc::Sender<Arc<Mutex<Coin>>>,
    pub coins_to_buy: Arc<Mutex<mpsc::Receiver<Arc<Mutex<Coin>>>>>,
    pub coins_to_sell: Arc<Mutex<mpsc::Receiver<String>>>,

    pub skip_ata_lookup: bool,
    pub blockhash: Arc<Mutex<Option<Hash>>>,
    pub jito_manager: Arc<JitoManager>,

    pub global_address: Pubkey,
    pub fee_id: Pubkey,
    pub event_authority: Pubkey,
    pub program_id: Pubkey,
    pub status_sender: Option<mpsc::Sender<String>>,
    pub geyser_endpoint: String,
    pub geyser_x_token: Option<String>,
    pub concurrent_coin_limit: Arc<Semaphore>,
    pub max_concurrent_coins: u32,
    pub is_monitoring: Arc<Mutex<bool>>,
}

/* 
impl fmt::Debug for Bot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bot")
            .field("private_key", &self.private_key)
            .field("fee_micro_lamports", &self.fee_micro_lamports)
            .field("buy_amount_lamports", &self.buy_amount_lamports)
            .field("skip_ata_lookup", &self.skip_ata_lookup)
            .field("global_address", &self.global_address)
            .field("fee_id", &self.fee_id)
            .field("event_authority", &self.event_authority)
            .field("program_id", &self.program_id)
            .finish()
    }
}
*/


impl Clone for Bot {
    fn clone(&self) -> Self {
        Self {
            rpc_client: self.rpc_client.clone(),
            ws_client: self.ws_client.clone(),
            send_tx_clients: self.send_tx_clients.clone(),
            private_key: self.private_key.clone(),
            db_connection: self.db_connection.clone(),
            fee_micro_lamports: self.fee_micro_lamports,
            buy_amount_lamports: self.buy_amount_lamports,
            pending_coins: self.pending_coins.clone(),
            coins_to_buy_sender: self.coins_to_buy_sender.clone(),
            coins_to_buy: self.coins_to_buy.clone(),
            coins_to_sell: self.coins_to_sell.clone(),
            skip_ata_lookup: self.skip_ata_lookup,
            blockhash: self.blockhash.clone(),
            jito_manager: self.jito_manager.clone(),
            global_address: self.global_address,
            fee_id: self.fee_id,
            event_authority: self.event_authority,
            program_id: self.program_id,
            status_sender: self.status_sender.clone(),
            geyser_endpoint: self.geyser_endpoint.clone(),
            geyser_x_token: self.geyser_x_token.clone(),
            concurrent_coin_limit: self.concurrent_coin_limit.clone(),
            max_concurrent_coins: self.max_concurrent_coins,
            is_monitoring: self.is_monitoring.clone(),
        }
    }
}

/// Structure representing a coin and its state
pub struct Coin {
    pub pickup_time: Instant,

    pub mint_address: Option<Pubkey>,
    pub token_bonding_curve: Pubkey,
    pub associated_bonding_curve: Pubkey,
    pub event_authority: Pubkey,

    pub creator: Pubkey,
    pub creator_ata: Option<Pubkey>,
    pub creator_purchased: bool,
    pub creator_purchase_sol: f64,

    pub creator_sold: bool,
    pub bot_purchased: bool,

    pub exited_buy_coin: bool,
    pub exited_sell_coin: bool,
    pub exited_creator_listener: bool,

    pub is_selling_coin: bool,

    pub associated_token_account: Option<Pubkey>,
    pub tokens_held: u64,

    pub buy_price: u64,
    pub buy_transaction_signature: Option<String>,
    pub mint_timestamp: i64,
    pub bonding_curve_data: Option<BondingCurveData>,
    pub wallet_tracking: bool,
}

impl Default for Coin {
    fn default() -> Self {
        Self {
            pickup_time: Instant::now(),
            mint_address: None,
            token_bonding_curve: Pubkey::default(),
            associated_bonding_curve: Pubkey::default(),
            event_authority: Pubkey::default(),
            creator: Pubkey::default(),
            creator_ata: None,
            creator_purchased: false,
            creator_purchase_sol: 0.0,
            creator_sold: false,
            bot_purchased: false,
            exited_buy_coin: false,
            exited_sell_coin: false,
            exited_creator_listener: false,
            is_selling_coin: false,
            associated_token_account: None,
            tokens_held: 0,
            buy_price: 0,
            buy_transaction_signature: None,
            mint_timestamp: 0,
            bonding_curve_data: Some(BondingCurveData::default()),
            wallet_tracking: false,
        }
    }
} 
unsafe impl Send for Coin {}
unsafe impl Sync for Coin {}

// Create a type alias for the default Bot type with InterceptorXToken


impl Bot {
    fn create_proxied_client(endpoint: &str, proxy_url: &str) -> Result<NonblockingRpcClient, BotError> {
        let http_client = Client::builder()
            .proxy(Proxy::all(proxy_url)
                .map_err(|e| BotError::RpcError(format!("Failed to create proxy: {}", e)))?)
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| BotError::RpcError(format!("Failed to build HTTP client: {}", e)))?;

        Ok(NonblockingRpcClient::new_with_timeout_and_commitment(
            endpoint.to_string(),
            Duration::from_secs(60),
            CommitmentConfig::confirmed()
        ))
    }


    /// Creates a new Bot instance
    pub async fn new(
        rpc_url: &str,
        ws_url: &str,
        private_key: &str,
        db_connection: MySqlPool,
        buy_sol: f64,
        fee_micro_lamports: u64,
        should_proxy: bool,
        proxy_url: Option<&str>,
        geyser_endpoint: &str,
        geyser_x_token: Option<&str>,
        jito_validator_api: Option<&str>,
        jito_validator_endpoint: Option<&str>,
        max_concurrent_coins: u32,
        fee_id: Pubkey
    ) -> Result<Self, BotError> {
        // Check if database connection is valid
        if db_connection.is_closed() {
            return Err(BotError::DbConnectionError("MySQL DB Connection is closed".to_string()));
        }

        // Create RPC client based on proxy settings
        let rpc_client = if should_proxy {
            if let Some(proxy) = proxy_url {
                Arc::new(Self::create_proxied_client(rpc_url, proxy)?)
            } else {
                return Err(BotError::RpcError("Proxy URL required when should_proxy is true".to_string()));
            }
        } else {
            Arc::new(NonblockingRpcClient::new_with_timeout_and_commitment(
                rpc_url.to_string(),
                Duration::from_secs(60),
                CommitmentConfig::confirmed()
            ))
        };


        // Create WebSocket client
        let ws_client = Arc::new(PubsubClient::new(ws_url).await
            .map_err(|e| BotError::WsError(e.to_string()))?);

        // Initialize gRPC client with default settings


        // Parse private key
        let private_key = Arc::new(Keypair::from_base58_string(private_key));

     
        // Convert SOL to lamports
        let buy_amount_lamports = (buy_sol * solana_sdk::native_token::LAMPORTS_PER_SOL as f64) as u64;

        // Create Jito manager
        let jito_manager = Arc::new(JitoManager::new(
            Arc::new(solana_client::rpc_client::RpcClient::new(rpc_url.to_string())),
            private_key.clone(),
            Arc::new(jito_validator_api.map(|s| s.to_string())),
            Arc::new(jito_validator_endpoint.map(|s| s.to_string()))
        ).await?);

        // Create send transaction clients
        let send_tx_clients = vec![rpc_client.clone()]; // Add more if needed

        // Create channels
        let (coins_to_buy_tx, coins_to_buy_rx) = mpsc::channel(100);
        let (coins_to_sell_tx, coins_to_sell_rx) = mpsc::channel(100);

        let bot = Self {
            rpc_client,
            ws_client,
            send_tx_clients,
            private_key,
            db_connection,
            fee_micro_lamports,
            buy_amount_lamports,
            pending_coins: Arc::new(Mutex::new(HashMap::new())),
            coins_to_buy_sender: coins_to_buy_tx,
            coins_to_buy: Arc::new(Mutex::new(coins_to_buy_rx)),
            coins_to_sell: Arc::new(Mutex::new(coins_to_sell_rx)),
            skip_ata_lookup: false,
            blockhash: Arc::new(Mutex::new(None)),
            jito_manager,
            global_address: GLOBAL_ID,
            fee_id: fee_id,
            event_authority: EVENT_AUTH_ID,
            program_id: PROGRAM_ID,
            status_sender: None,
            geyser_endpoint: geyser_endpoint.to_string(),
            geyser_x_token: geyser_x_token.map(|s| s.to_string()),
            concurrent_coin_limit: Arc::new(Semaphore::new(max_concurrent_coins as usize)),
            max_concurrent_coins,
            is_monitoring: Arc::new(Mutex::new(true)),
        };

        // Start blockhash loop
        bot.start_blockhash_loop().await;

        Ok(bot)
    }

    pub fn status(&self, msg: &str) {
        info!("Bot: {}", msg);
    }

    pub fn status_yellow(&self, msg: &str) {
        warn!("Bot (Y): {}", msg);
    }

    pub fn status_green(&self, msg: &str) {
        info!("Bot (G): {}", msg);
    }

    pub fn status_red(&self, msg: &str) {
        error!("Bot (R): {}", msg);
    }

    /// Checks if we should stop monitoring based on pending coins count
    pub async fn check_monitoring_state(&self) {
        let pending_count = self.pending_coins.lock().await.len();
        let mut monitoring = self.is_monitoring.lock().await;

        if pending_count >= self.max_concurrent_coins as usize {
            if *monitoring {
                *monitoring = false;
                info!("Already monitoring {} coins (limit: {}). Stopping RPC and gRPC monitoring...", 
                    pending_count, self.max_concurrent_coins);
            }
        } else {
            if !*monitoring {
                *monitoring = true;
                info!("Pending coins ({}) below limit ({}), resuming RPC and gRPC monitoring...", 
                    pending_count, self.max_concurrent_coins);
            }
        }
    }
}
impl Coin {
    pub fn status(&self, msg: &str) {
        if let Some(mint) = self.mint_address {
            info!("{}: {}", mint, msg);
        }
    }

    pub fn bot_holds_tokens(&self) -> bool {
        self.bot_purchased && self.tokens_held > 0
    }
}

