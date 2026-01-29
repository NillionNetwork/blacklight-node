use alloy::primitives::U256;
use opentelemetry::{
    global,
    metrics::{Gauge, Meter},
};
use std::sync::LazyLock;

static METRICS: LazyLock<Metrics> = LazyLock::new(|| {
    let meter = global::meter("heartbeat-funder");
    Metrics::new(&meter)
});

pub(crate) fn get() -> &'static Metrics {
    &METRICS
}

pub(crate) struct Metrics {
    pub(crate) l1: L1Metrics,
    // A private guard to prevent this type from being constructed outside of this module.
    _private: (),
}

impl Metrics {
    fn new(meter: &Meter) -> Self {
        let l1 = L1Metrics::new(meter);
        Self { l1, _private: () }
    }
}

pub(crate) struct L1Metrics {
    pub(crate) eth: L1EthMetrics,
    pub(crate) epochs: L1EpochsMetrics,
}

impl L1Metrics {
    fn new(meter: &Meter) -> Self {
        let eth = L1EthMetrics::new(meter);
        let epochs = L1EpochsMetrics::new(meter);
        Self { eth, epochs }
    }
}

pub(crate) struct L1EthMetrics {
    funds: Gauge<f64>,
}

impl L1EthMetrics {
    fn new(meter: &Meter) -> Self {
        let funds = meter
            .f64_gauge("blacklight.keeper.l1.eth.total")
            .with_description("Total amount of ETH available in L1 wallet")
            .with_unit("ETH")
            .build();
        Self { funds }
    }

    pub(crate) fn set_funds(&self, amount: U256) {
        self.funds.record(amount.into(), &[]);
    }
}

pub(crate) struct L1EpochsMetrics {
    minted: Gauge<u64>,
    total: Gauge<u64>,
}

impl L1EpochsMetrics {
    fn new(meter: &Meter) -> Self {
        let minted = meter
            .u64_gauge("blacklight.keeper.l1.epochs.minted")
            .with_description("Total minted epochs")
            .build();
        let total = meter
            .u64_gauge("blacklight.keeper.l1.epochs.total")
            .with_description("Total epochs")
            .build();
        Self { minted, total }
    }

    pub(crate) fn set_total(&self, amount: U256) {
        let Ok(amount) = amount.try_into() else {
            return;
        };
        self.total.record(amount, &[]);
    }

    pub(crate) fn set_minted(&self, amount: U256) {
        let Ok(amount) = amount.try_into() else {
            return;
        };
        self.minted.record(amount, &[]);
    }
}
