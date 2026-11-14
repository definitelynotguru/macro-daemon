//! Public surface of the e2e harness: the stats module is what the integration
//! tests need to reach without going through the binary.
pub mod stats;

pub use stats::Stats;
