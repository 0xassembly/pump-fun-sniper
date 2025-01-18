use {
    anyhow::{Result, anyhow},
    dotenv::dotenv,
    log::{info, error},
    sqlx::{MySql, Pool},
    solana_sdk::{
        signature::Keypair,
        signer::Signer,
    },
    std::{
        env,
        str::FromStr,
        sync::Arc,
    },
    tokio,
    bot::constants::{DEVNET_FEE_ID, MAINNET_FEE_ID},
};

use crate::bot::Bot;

mod bot;
mod jito;
mod pump;
mod metrics;
pub mod error;

async fn load_private_key() -> Result<Keypair> {
    dotenv().ok();
    
    let private_key = env::var("PRIVATE_KEY")
        .map_err(|_| anyhow!("PRIVATE_KEY not found in environment"))?;
        
    Ok(Keypair::from_base58_string(&private_key))
}

async fn setup_database() -> Result<Pool<MySql>> {
    let database_url = "mysql://root:12345@localhost/CoinTrades";
    
    Pool::connect(database_url)
        .await
        .map_err(|e| anyhow!("Failed to connect to database: {}", e))
}

async fn clear_database(db: &Pool<MySql>) -> Result<()> {
    info!("Clearing database tables...");
    
    // List of tables to clear
    let tables = ["coins"];
    
    for table in tables {
        sqlx::query(&format!("TRUNCATE TABLE {}", table))
            .execute(db)
            .await
            .map_err(|e| anyhow!("Failed to clear table {}: {}", table, e))?;
        
        info!("Cleared table: {}", table);
    }
    
    info!("Database cleared successfully");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging with debug level for our crate
    std::env::set_var("RUST_LOG", "info,second_pump_fun_sniper_bot=debug");
    env_logger::init();
    
    info!("Starting bot...");
    
    // Load environment variables
    dotenv().ok();
    
    // Determine which monitoring system to use (RPC or gRPC)
    let use_grpc = env::var("USE_GRPC")
        .map(|val| val.to_lowercase() == "true")
        .unwrap_or(false);
    
    info!("Using gRPC monitoring: {}", use_grpc);

    let jito_api_endpoint = env::var("JITO_API_ENDPOINT")
        .map_err(|_| anyhow!("JITO_API_ENDPOINT not found in environment"))?;

    let jito_validator_endpoint = env::var("JITO_VALIDATOR_ENDPOINT")
        .map_err(|_| anyhow!("JITO_VALIDATOR_ENDPOINT not found in environment"))?;

    let use_devnet = env::var("IS_DEVNET")
    .map(|val| val.to_lowercase() == "true")
    .unwrap_or(false);

    let (rpc_env, ws_env, fee_id) = if use_devnet {
        ("DEVNET_RPC_URL",
         "DEVNET_WS_URL",  
          DEVNET_FEE_ID
        )
    } else {
        ("MAINNET_RPC_URL",
         "MAINNET_WS_URL",
         MAINNET_FEE_ID
         )
    };

    let (rpc_url, ws_url) = (
        env::var(rpc_env).map_err(|_| anyhow!("{} not found in environment", rpc_env))?,
        env::var(ws_env).map_err(|_| anyhow!("{} not found in environment", ws_env))?
    );

    // Get Geyser URL
    let grpc_endpoint = env::var("GRPC_ENDPOINT")
        .map_err(|_| anyhow!("GRPC_URL not found in environment"))?;
    
    let grpc_x_token = env::var("GRPC_X_TOKEN")
        .map_err(|_| anyhow!("GRPC_X_TOKEN not found in environment"))?;

    // Check if proxy is enabled
    let should_proxy: bool = env::var("PROXY_URL")
        .map(|url| url.contains("http"))
        .unwrap_or(false);
    
    // Get proxy URL if enabled
    let proxy_url = if should_proxy {
        Some(env::var("PROXY_URL").unwrap())
    } else {
        None
    };
    
    // Setup database connection
    let db: Pool<MySql> = setup_database().await?;
    
    // Clear database tables
    clear_database(&db).await?;
    
    // Load private key
    let keypair = load_private_key().await?;
    
    // Create send tx RPCs list
    let send_tx_rpcs: Vec<String> = Vec::new(); // Add public RPCs here if needed
    
    // Create bot with 0.05 SOL purchase amount and 1,000,000 microlamports priority fee
    let mut bot = Bot::new(
        &rpc_url,
        &ws_url,
        &keypair.to_base58_string(),
        db,
        0.0275,
        1_000_000,
        proxy_url.is_some(),
        proxy_url.as_deref(),
        &grpc_endpoint,
        Some(&grpc_x_token.as_str()),
        Some(&jito_api_endpoint),
        Some(&jito_validator_endpoint),
        1,
        fee_id
    ).await?;
    
    // Set ATA lookup flag
    bot.skip_ata_lookup = true;
    
    // Initialize Jito
    if let Err(e) = bot.jito_manager.start().await {
        error!("Failed to start Jito: {}", e);
        return Err(anyhow!("Error starting Jito: {}", e));
    }

    // Spawn monitoring handler based on configuration
    let bot_clone = bot.clone();
    tokio::spawn(async move {
        let result = if use_grpc {
            info!("Starting gRPC monitoring...");
            bot_clone.grpc_monitor_mints().await
        } else {
            info!("Starting RPC monitoring...");
            bot_clone.rpc_monitor_mints().await
        };
        
        if let Err(e) = result {
            error!("Error in monitoring handler: {}", e);
        }
    });
    
    let bot_clone = bot.clone();
    tokio::spawn(async move {
        let result = if use_grpc {
            bot_clone.handle_buy_coins(true).await
        } else {
            bot_clone.handle_buy_coins(false).await
        };
        
        if let Err(e) = result {
            error!("Error in buy coins handler: {}", e);
        }
    });
    
    let bot_clone = bot.clone();
    tokio::spawn(async move {
        bot_clone.handle_sell_coins().await;
        // Ensure the closure returns ()
        ()
    });
    
    info!("Bot started successfully");
    
    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    Ok(())
}
