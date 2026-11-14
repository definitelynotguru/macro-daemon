//! Public surface of the e2e harness: the stats module is what the integration
//! tests need to reach without going through the binary.
pub mod measure;
pub mod stats;

pub use measure::TimestampingInjector;
pub use stats::Stats;
