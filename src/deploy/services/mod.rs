//! Service modules for the OP Stack deployment.
//!
//! Each service is in its own submodule with:
//! - `cmd.rs` - Command builder for generating Docker commands
//! - `mod.rs` - Config, Handler, and start logic

pub mod anvil;
pub mod kona_node;
pub mod op_batcher;
pub mod op_challenger;
pub mod op_proposer;
pub mod op_reth;

// Re-export commonly used types
pub use anvil::{AnvilConfig, AnvilHandler};
pub use kona_node::{KonaNodeConfig, KonaNodeHandler};
pub use op_batcher::{OpBatcherConfig, OpBatcherHandler};
pub use op_challenger::{OpChallengerConfig, OpChallengerHandler};
pub use op_proposer::{OpProposerConfig, OpProposerHandler};
pub use op_reth::{OpRethConfig, OpRethHandler};

