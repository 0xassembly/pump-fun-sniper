use {
    lazy_static::lazy_static,
    prometheus::{
        Counter, CounterVec, Gauge, GaugeVec, Histogram, HistogramVec,
        IntCounter, IntCounterVec, IntGauge, IntGaugeVec,
        Opts, Registry,
    },
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // Transaction metrics
    pub static ref TRANSACTIONS_TOTAL: IntCounter = 
        IntCounter::new("transactions_total", "Total number of transactions processed").unwrap();
    
    pub static ref TRANSACTION_ERRORS: IntCounterVec = 
        IntCounterVec::new(
            Opts::new("transaction_errors", "Number of transaction errors"),
            &["error_type"]
        ).unwrap();

    // Coin metrics
    pub static ref COINS_BOUGHT: IntCounter = 
        IntCounter::new("coins_bought", "Total number of coins bought").unwrap();
    
    pub static ref COINS_SOLD: IntCounter = 
        IntCounter::new("coins_sold", "Total number of coins sold").unwrap();

    pub static ref COIN_PRICE: GaugeVec = 
        GaugeVec::new(
            Opts::new("coin_price", "Current coin price in SOL"),
            &["coin_address"]
        ).unwrap();

    // Performance metrics
    pub static ref TRANSACTION_LATENCY: Histogram = 
        Histogram::with_opts(
            Opts::new("transaction_latency", "Transaction latency in milliseconds").into()
        ).unwrap();

    pub static ref RPC_LATENCY: HistogramVec = 
        HistogramVec::new(
            Opts::new("rpc_latency", "RPC request latency in milliseconds").into(),
            &["method"]
        ).unwrap();

    // System metrics
    pub static ref MEMORY_USAGE: Gauge = 
        Gauge::new("memory_usage", "Current memory usage in bytes").unwrap();
    
    pub static ref GOROUTINE_COUNT: IntGauge = 
        IntGauge::new("goroutine_count", "Number of active goroutines").unwrap();
}

// Register all metrics
pub fn register_metrics() {
    REGISTRY.register(Box::new(TRANSACTIONS_TOTAL.clone())).unwrap();
    REGISTRY.register(Box::new(TRANSACTION_ERRORS.clone())).unwrap();
    REGISTRY.register(Box::new(COINS_BOUGHT.clone())).unwrap();
    REGISTRY.register(Box::new(COINS_SOLD.clone())).unwrap();
    REGISTRY.register(Box::new(COIN_PRICE.clone())).unwrap();
    REGISTRY.register(Box::new(TRANSACTION_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(RPC_LATENCY.clone())).unwrap();
    REGISTRY.register(Box::new(MEMORY_USAGE.clone())).unwrap();
    REGISTRY.register(Box::new(GOROUTINE_COUNT.clone())).unwrap();
}
