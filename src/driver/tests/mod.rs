//! Driver integration tests split by compiler stage (rewrite phase 0).

mod baseline;
mod cli;
mod elab_only;
mod new_pipeline;
mod quantum_pipeline;
mod parse_only;
mod resolve_only;
mod typecheck_only;
