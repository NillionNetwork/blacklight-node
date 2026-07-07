use alloy::primitives::U256;
use opentelemetry::{
    KeyValue, global,
    metrics::{Counter, Gauge, Meter},
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
    pub(crate) l2: L2Metrics,
    // A private guard to prevent this type from being constructed outside of this module.
    _private: (),
}

impl Metrics {
    fn new(meter: &Meter) -> Self {
        let l1 = L1Metrics::new(meter);
        let l2 = L2Metrics::new(meter);
        Self {
            l1,
            l2,
            _private: (),
        }
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
    blocked: Gauge<u64>,
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
        let blocked = meter
            .u64_gauge("blacklight.keeper.l1.epochs.blocked")
            .with_description(
                "Whether epochs are blocked due to the spendable budget not being depleted",
            )
            .build();
        Self {
            minted,
            total,
            blocked,
        }
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

    pub(crate) fn set_blocked(&self, value: bool) {
        let value = if value { 1 } else { 0 };
        self.blocked.record(value, &[]);
    }
}

pub(crate) struct L2Metrics {
    pub(crate) events: L2EventMetrics,
    pub(crate) rewards: L2RewardsMetrics,
    pub(crate) escalations: L2EscalationsMetrics,
    pub(crate) eth: L2EthMetrics,
    pub(crate) erc8004: L2Erc8004Metrics,
}

impl L2Metrics {
    fn new(meter: &Meter) -> Self {
        let events = L2EventMetrics::new(meter);
        let rewards = L2RewardsMetrics::new(meter);
        let escalations = L2EscalationsMetrics::new(meter);
        let eth = L2EthMetrics::new(meter);
        let erc8004 = L2Erc8004Metrics::new(meter);
        Self {
            events,
            rewards,
            escalations,
            eth,
            erc8004,
        }
    }
}

pub(crate) struct L2EventMetrics {
    received: Counter<u64>,
}

impl L2EventMetrics {
    fn new(meter: &Meter) -> Self {
        let received = meter
            .u64_counter("blacklight.keeper.l2.events.received")
            .with_description("Total L2 events received")
            .build();
        Self { received }
    }

    pub(crate) fn inc_events_received(&self, name: &'static str) {
        self.received.add(1, &[KeyValue::new("name", name)]);
    }
}

pub(crate) struct L2RewardsMetrics {
    distribution: Counter<u64>,
    spendable: Gauge<f64>,
    remaining: Gauge<f64>,
    end: Gauge<u64>,
}

impl L2RewardsMetrics {
    fn new(meter: &Meter) -> Self {
        let distribution = meter
            .u64_counter("blacklight.keeper.l2.rewards.distributions")
            .with_description("Number of times rewards were distributed")
            .build();
        let spendable = meter
            .f64_gauge("blacklight.keeper.l2.rewards.spendable")
            .with_description("The spendable budget for rewards")
            .build();
        let remaining = meter
            .f64_gauge("blacklight.keeper.l2.rewards.remaining")
            .with_description("The remaining budget to be unlocked for rewards")
            .build();
        let end = meter
            .u64_gauge("blacklight.keeper.l2.rewards.end")
            .with_description("The UNIX epoch timestamp when the current budget runs out")
            .build();
        Self {
            distribution,
            spendable,
            remaining,
            end,
        }
    }

    pub(crate) fn inc_distributions(&self) {
        self.distribution.add(1, &[]);
    }

    pub(crate) fn set_spendable(&self, value: U256) {
        self.spendable.record(value.into(), &[]);
    }

    pub(crate) fn set_remaining(&self, value: U256) {
        self.remaining.record(value.into(), &[]);
    }

    pub(crate) fn set_end(&self, value: u64) {
        self.end.record(value, &[]);
    }
}

pub(crate) struct L2EscalationsMetrics {
    total: Counter<u64>,
    block: Gauge<u64>,
    round_starts: Counter<u64>,
}

impl L2EscalationsMetrics {
    fn new(meter: &Meter) -> Self {
        let total = meter
            .u64_counter("blacklight.keeper.l2.escalations.total")
            .with_description("Total number of escalations")
            .build();
        let block = meter
            .u64_gauge("blacklight.keeper.l2.escalations.block")
            .with_description("The block used for escalations")
            .build();
        let round_starts = meter
            .u64_counter("blacklight.keeper.l2.round_starts.total")
            .with_description("Total number of startRound cranks (C4a split)")
            .build();
        Self {
            total,
            block,
            round_starts,
        }
    }

    pub(crate) fn inc_escalations(&self) {
        self.total.add(1, &[]);
    }

    pub(crate) fn inc_round_starts(&self) {
        self.round_starts.add(1, &[]);
    }

    pub(crate) fn set_block(&self, block: u64) {
        self.block.record(block, &[]);
    }
}

pub(crate) struct L2EthMetrics {
    funds: Gauge<f64>,
}

impl L2EthMetrics {
    fn new(meter: &Meter) -> Self {
        let funds = meter
            .f64_gauge("blacklight.keeper.l2.eth.total")
            .with_description("Total amount of ETH available in L2 wallet")
            .with_unit("ETH")
            .build();
        Self { funds }
    }

    pub(crate) fn set_funds(&self, amount: U256) {
        self.funds.record(amount.into(), &[]);
    }
}

pub(crate) struct L2Erc8004Metrics {
    requests_tracked: Gauge<u64>,
    responses_submitted: Counter<u64>,
}

impl L2Erc8004Metrics {
    fn new(meter: &Meter) -> Self {
        let requests_tracked = meter
            .u64_gauge("blacklight.keeper.l2.erc8004.requests_tracked")
            .with_description("Number of ERC-8004 validation requests currently tracked")
            .build();
        let responses_submitted = meter
            .u64_counter("blacklight.keeper.l2.erc8004.responses_submitted")
            .with_description("Total ERC-8004 validation responses submitted")
            .build();
        Self {
            requests_tracked,
            responses_submitted,
        }
    }

    pub(crate) fn set_requests_tracked(&self, count: u64) {
        self.requests_tracked.record(count, &[]);
    }

    pub(crate) fn inc_responses_submitted(&self) {
        self.responses_submitted.add(1, &[]);
    }
}
