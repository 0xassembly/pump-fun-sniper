use std::sync::Arc;
use anyhow::Result;
use warp::{Filter, Reply};
use prometheus::{Encoder, TextEncoder};

use super::types::REGISTRY;
use super::collector::MetricsCollector;

pub struct MetricsExporter {
    collector: Arc<MetricsCollector>,
    port: u16,
}

impl MetricsExporter {
    pub fn new(collector: MetricsCollector, port: u16) -> Self {
        Self {
            collector: Arc::new(collector),
            port,
        }
    }

    pub async fn start(self) -> Result<()> {
        let metrics_route = warp::path!("metrics")
            .map(|| {
                let encoder = TextEncoder::new();
                let metric_families = REGISTRY.gather();
                let mut buffer = vec![];
                encoder.encode(&metric_families, &mut buffer).unwrap();
                String::from_utf8(buffer).unwrap()
            })
            .map(|reply| warp::reply::with_header(reply, "Content-Type", "text/plain"));

        println!("Starting metrics server on port {}", self.port);
        
        warp::serve(metrics_route)
            .run(([127, 0, 0, 1], self.port))
            .await;

        Ok(())
    }
}
