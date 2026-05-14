//! Operator JSON-RPC console (merged from reth-console).

mod cli;
mod command;
mod endpoint;
mod engine;
mod exec;
mod output;
mod query;
mod repl;
mod rpc;
/// Method suffix tables for tab-completing `namespace.method` in the REPL.
mod rpc_completion;
mod run;

pub use cli::ConsoleCommand;
