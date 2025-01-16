use {
    crate::bot::{Bot, structs::Coin},
    log::{info},
    std::{sync::Arc, time::Duration},
    tokio::sync::Mutex,
    futures::future::join_all,
    tokio::time::sleep
};

impl Bot {
    /// Handles selling coins by continuously monitoring pending coins
    /// and initiating sell operations when necessary
    pub async fn handle_sell_coins(&self) {
        loop {
            // Process pending coins
            let coins_to_sell = self.fetch_coins_to_sell().await;
            if coins_to_sell.is_empty() {
                tokio::time::sleep(Duration::from_millis(10)).await;
                continue;
            }

            // Process each coin that needs to be sold
            for coin in coins_to_sell {
                let bot = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = bot.sell_coin_fast(coin).await {
                        info!("Error selling coin: {}", e);
                    }
                });
            }

            // Check if we need to update monitoring state
            self.check_monitoring_state().await;

            // Wait before next check (100ms like in Go version)
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// Fetches coins that should be sold and cleans up tracked coins
    async fn fetch_coins_to_sell(&self) -> Vec<Arc<Mutex<Coin>>> {
        let mut coins_to_sell = Vec::new();
        let mut pending_coins = self.pending_coins.lock().await;
        let mut to_remove = Vec::new();
        // Check monitoring state on each message

        //info!("Fetching coins to sell");
        // First collect all coins and their states
        for (mint_addr, coin) in pending_coins.iter() {
            let coin_guard = coin.lock().await;

            if coin_guard.exited_buy_coin && !coin_guard.bot_holds_tokens() {
                info!("Will delete {} because exited buy but no hold", coin_guard.mint_address.unwrap());
                to_remove.push(mint_addr.clone());
            } else if coin_guard.exited_sell_coin && coin_guard.exited_creator_listener {
                info!("Will delete {} because exited creator listener and sellCoins routine", 
                    coin_guard.mint_address.unwrap());
                to_remove.push(mint_addr.clone());
            } else if coin_guard.bot_holds_tokens() && 
                      coin_guard.creator_sold && 
                      !coin_guard.is_selling_coin {
                info!("Will sell {}: (decision=creator sold)", coin_guard.mint_address.unwrap());
                coins_to_sell.push(Arc::clone(coin));
            }
        }

        // Remove coins that need to be removed
        for mint_addr in to_remove {
            pending_coins.remove(&mint_addr);
        }

        coins_to_sell
    }
} 