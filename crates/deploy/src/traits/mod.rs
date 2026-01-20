//! Trait-based service architecture for Kupcake deployment.
//!
//! This module provides a trait-based approach to deploying services in a type-safe manner.
//! The deployment process follows a fixed stage order: L1 -> Contracts -> L2 -> Monitoring.
//!
//! # Example
//!
//! ```no_run
//! use kupcake_deploy::{Deployer, AnvilConfig, OpDeployerConfig, L2StackBuilder, MonitoringConfig};
//!
//! let deployer = Deployer::new(AnvilConfig::default())
//!     .then(OpDeployerConfig::default())
//!     .then(L2StackBuilder::default())
//!     .then(MonitoringConfig::default());
//! ```

mod context;
mod deployer;
mod runner;
mod service;
mod stages;
mod standard;

pub use context::{ContractsContext, L1Context, L2Context, MonitoringContext};
pub use deployer::{Deployer, End};
pub use runner::DeployChain;
pub use service::KupcakeService;
pub use stages::{ContractsStage, DeploymentStage, L1Stage, L2Stage, MonitoringStage, NextStage};
pub use standard::{NoMonitoringDeployer, StandardDeployer, StandardDeploymentResult};
