//! zakhar core library.
//!
//! The zakhar engine, exposed as a library so it can be embedded (e.g. the
//! mobile companion app) as well as driven by the CLI binary. The CLI in
//! `main.rs` is a thin shell over these modules; behaviour is identical.
//!
//! See `Cargo.toml` — both a `[lib]` (this crate) and a `[[bin]]` (CLI) are
//! produced from the same sources.

pub mod agent;
pub mod cli;
pub mod config;
pub mod delegate;
pub mod handler;
pub mod hooks;
pub mod invoke;
pub mod memory;
pub mod provider;
pub mod registry;
pub mod session;
pub mod slash;
pub mod tools;
pub mod types;
pub mod ui;
