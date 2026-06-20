pub mod caching;
pub mod classification;
pub mod cli;
pub mod config;
pub mod counts;
pub mod date;
pub mod dirty;
pub mod discovery;
pub mod error;
pub mod fetch;
pub mod filter;
pub mod git;
pub mod gix_ops;
pub mod landed;
pub mod output;
pub mod progress;
pub mod scan;
pub mod types;

#[cfg(any(test, feature = "testutil"))]
pub mod testutil;
