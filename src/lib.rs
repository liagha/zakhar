//! zakhar core library.
//!
//! The zakhar engine, exposed as a library so it can be embedded (e.g. the
//! mobile companion app) as well as driven by the CLI binary. The CLI in
//! `main.rs` is a thin shell over these modules; behaviour is identical.
//!
//! See `Cargo.toml` — both a `[lib]` (this crate) and a `[[bin]]` (CLI) are
//! produced from the same sources.

pub mod agent;
#[cfg(feature = "jni")]
pub mod android;
pub mod cli;
pub mod config;
pub mod delegate;
pub mod handler;
pub mod hooks;
pub mod invoke;
pub mod ledger;
pub mod memory;
pub mod migrate;
pub mod mobile;
pub mod paths;
pub mod provider;
pub mod registry;
pub mod reminder;
pub mod session;
pub mod slash;
pub mod term;
pub mod tools;
pub mod types;
pub mod ui;
