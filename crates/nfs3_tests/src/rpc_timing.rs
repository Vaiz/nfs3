//! B0.6 normal-system-allocator cold-connection, tapped-harness latency.
//! The operation window includes server first poll/startup, transcript copies and
//! first-touch faults, duplex growth/backpressure, argument construction, backend
//! work, scheduling and success mapping. Only the server spawn call is excluded,
//! not its execution. Post-send work included varies with scheduling.
//! Fixture/runtime creation, post-response validation, output drop and teardown
//! are excluded. This is not steady-state service time or TCP/network throughput.
//! No tracing subscriber or allocator recorder is installed. Full feature-lane
//! overhead is distinct from isolated hook cost; recorder overhead remains unknown.
//! Callers MUST use an external process watchdog (see `rpc_baseline`).

use std::time::{Duration, Instant};

use anyhow::{Result, ensure};

use crate::rpc_baseline::{Case, Observer, run_observed};

pub const MAX_WARMUPS: usize = 32;
pub const MAX_SAMPLES: usize = 512;

/// Finite per-process workload. Options may appear once each, after CASE.
#[derive(Debug)]
pub struct Config {
    pub case: String,
    pub warmups: usize,
    pub samples: usize,
}

impl Config {
    pub fn parse(args: &[String]) -> Result<Self> {
        let Some(case) = args.first() else {
            anyhow::bail!("usage: rpc_timing CASE [--warmup 0..32] [--samples 1..512]");
        };
        let mut config = Self {
            case: case.clone(),
            warmups: MAX_WARMUPS,
            samples: MAX_SAMPLES,
        };
        let mut warmup_seen = false;
        let mut samples_seen = false;
        let mut options = args[1..].chunks_exact(2);
        for pair in &mut options {
            match pair[0].as_str() {
                "--warmup" => {
                    ensure!(!warmup_seen, "duplicate --warmup");
                    warmup_seen = true;
                    config.warmups = pair[1].parse()?;
                }
                "--samples" => {
                    ensure!(!samples_seen, "duplicate --samples");
                    samples_seen = true;
                    config.samples = pair[1].parse()?;
                }
                _ => anyhow::bail!("unknown option"),
            }
        }
        ensure!(options.remainder().is_empty(), "missing option value");
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        Case::parse(&self.case)?;
        ensure!(self.warmups <= MAX_WARMUPS, "warmup bound");
        ensure!((1..=MAX_SAMPLES).contains(&self.samples), "sample bound");
        Ok(())
    }

    fn payload_bytes(&self) -> Result<Option<u128>> {
        if self.case.starts_with("read-") || self.case.starts_with("write-") {
            let (_, kib) = self.case.split_once('-').expect("validated bulk case");
            Ok(Some(
                kib.parse::<u128>()?
                    .checked_mul(1024)
                    .ok_or_else(|| anyhow::anyhow!("payload overflow"))?,
            ))
        } else {
            Ok(None)
        }
    }
}

/// Checked integer nanoseconds; zero and unrepresentable intervals are failures.
pub fn nanoseconds(duration: Duration) -> Result<u64> {
    let ns = u64::try_from(duration.as_nanos())?;
    ensure!(ns != 0, "zero duration");
    Ok(ns)
}

/// Staged timestamps only: no sample is valid until the entire harness succeeds.
#[derive(Default)]
pub struct TimingObserver {
    start: Option<Instant>,
    end: Option<Instant>,
    invalid: bool,
}

impl TimingObserver {
    pub fn start_at(&mut self, now: Instant) {
        if self.start.is_some() || self.end.is_some() {
            self.invalid = true;
        }
        self.start = Some(now);
    }

    pub fn end_at(&mut self, now: Instant) {
        if self.start.is_none() || self.end.is_some() {
            self.invalid = true;
        }
        self.end = Some(now);
    }

    /// Validate boundaries only after golden, payload, ledger and teardown checks.
    pub fn finish(self, harness_result: Result<()>) -> Result<u64> {
        harness_result?;
        ensure!(!self.invalid, "duplicate or out-of-order timing boundary");
        let duration = self
            .end
            .zip(self.start)
            .and_then(|(end, start)| end.checked_duration_since(start))
            .ok_or_else(|| anyhow::anyhow!("missing or reversed timing boundary"))?;
        nanoseconds(duration)
    }
}

impl Observer for TimingObserver {
    const PRINT: bool = false;

    fn operation_start(&mut self) {
        self.start_at(Instant::now());
    }

    fn operation_end(&mut self) {
        self.end_at(Instant::now());
    }
}

/// Preallocated raw samples in operation order, never grown during capture.
pub struct Samples {
    values: Vec<u64>,
    capacity: usize,
    limit: usize,
}

impl Samples {
    pub fn new(limit: usize) -> Result<Self> {
        ensure!((1..=MAX_SAMPLES).contains(&limit), "sample bound");
        let values = Vec::with_capacity(limit);
        let capacity = values.capacity();
        Ok(Self {
            values,
            capacity,
            limit,
        })
    }

    pub fn commit(&mut self, observer: TimingObserver, result: Result<()>) -> Result<()> {
        let ns = observer.finish(result)?;
        ensure!(
            self.values.capacity() == self.capacity,
            "sample capacity changed"
        );
        ensure!(self.values.len() < self.limit, "sample storage full");
        self.values.push(ns);
        ensure!(
            self.values.capacity() == self.capacity,
            "sample capacity changed"
        );
        Ok(())
    }

    pub fn values(&self) -> &[u64] {
        &self.values
    }

    pub fn capacity(&self) -> usize {
        self.values.capacity()
    }
}

/// Integer floor units per second; all multiplication is checked u128.
pub fn rate(units: u128, ns: u128) -> Result<u128> {
    ensure!(ns != 0, "zero rate duration");
    Ok(units
        .checked_mul(1_000_000_000)
        .ok_or_else(|| anyhow::anyhow!("rate overflow"))?
        / ns)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Statistics {
    pub sum_ns: u128,
    pub p50_ns: u64,
    pub p95_ns: u64,
    pub p99_ns: u64,
    pub operation_window_ops_per_s: u128,
    pub operation_window_payload_bytes_per_s: Option<u128>,
    pub measured_loop_wall_ns: u64,
    pub measured_loop_wall_ops_per_s: u128,
}

/// Nearest-rank quantiles; sorting a separate copy preserves raw operation order.
pub fn statistics(samples: &[u64], payload: Option<u128>, wall_ns: u64) -> Result<Statistics> {
    ensure!((1..=MAX_SAMPLES).contains(&samples.len()), "sample bound");
    ensure!(samples.iter().all(|&ns| ns != 0), "zero sample");
    let sum_ns = samples.iter().try_fold(0_u128, |sum, &ns| {
        sum.checked_add(u128::from(ns))
            .ok_or_else(|| anyhow::anyhow!("sample sum overflow"))
    })?;
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let quantile = |percent: usize| sorted[(samples.len() * percent).div_ceil(100) - 1];
    let count = samples.len() as u128;
    let payload_rate = payload
        .map(|bytes| {
            let total = bytes
                .checked_mul(count)
                .ok_or_else(|| anyhow::anyhow!("payload total overflow"))?;
            rate(total, sum_ns)
        })
        .transpose()?;
    Ok(Statistics {
        sum_ns,
        p50_ns: quantile(50),
        p95_ns: quantile(95),
        p99_ns: quantile(99),
        operation_window_ops_per_s: rate(count, sum_ns)?,
        operation_window_payload_bytes_per_s: payload_rate,
        measured_loop_wall_ns: wall_ns,
        measured_loop_wall_ops_per_s: rate(count, u128::from(wall_ns))?,
    })
}

/// No output is published until every warmup, sample and statistic succeeds.
pub fn run(config: &Config) -> Result<(Samples, Statistics)> {
    ensure!(
        !cfg!(feature = "allocator-diagnostics"),
        "timing requires allocator-diagnostics disabled"
    );
    config.validate()?;
    let mut samples = Samples::new(config.samples)?;
    for _ in 0..config.warmups {
        let mut observer = TimingObserver::default();
        let result = run_observed(&config.case, &mut observer);
        observer.finish(result)?;
    }
    let wall_start = Instant::now();
    for _ in 0..config.samples {
        let mut observer = TimingObserver::default();
        let result = run_observed(&config.case, &mut observer);
        samples.commit(observer, result)?;
    }
    let wall_ns = nanoseconds(wall_start.elapsed())?;
    let stats = statistics(samples.values(), config.payload_bytes()?, wall_ns)?;
    Ok((samples, stats))
}
