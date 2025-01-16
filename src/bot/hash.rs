use {
    crate::bot::Bot,
    log::error,
    solana_sdk::commitment_config::CommitmentConfig,
    std::time::Duration,
};

impl Bot {
    /// Starts a background task that continuously updates the latest blockhash
    pub async fn start_blockhash_loop(&self) {
        let bot = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = bot.fetch_latest_blockhash().await {
                    error!("Error fetching blockhash: {}", e);
                    continue;
                }

                // Sleep for 400ms like in Go version
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        });
    }

    /// Fetches the latest blockhash from the Solana network
    async fn fetch_latest_blockhash(&self) -> Result<(), Box<dyn std::error::Error>> {
        let blockhash = self.rpc_client
            .get_latest_blockhash_with_commitment(CommitmentConfig::finalized())
            .await?
            .0;

        // Update the blockhash in the bot's state
        let mut hash = self.blockhash.lock().await;
        *hash = Some(blockhash);

        Ok(())
    }
} 