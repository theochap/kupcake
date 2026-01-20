//! Core service trait for Kupcake deployment.

use std::future::Future;
use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};

use super::stages::DeploymentStage;

/// Core trait for Kupcake services.
///
/// A service transforms configuration into a running handler through deployment.
/// The `Stage` associated type determines when in the deployment pipeline
/// this service can be deployed.
///
/// # Type Parameters
/// - `Stage`: The deployment stage this service belongs to
/// - `Handler`: Runtime handle to the deployed service
/// - `Context`: Stage-specific deployment context
pub trait KupcakeService: Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// The deployment stage this service belongs to.
    type Stage: DeploymentStage;

    /// The runtime handler type returned after deployment.
    type Handler: Send + 'static;

    /// The context type required for deployment.
    type Context<'a>;

    /// The name of this service for logging/identification.
    const SERVICE_NAME: &'static str;

    /// Deploy the service, returning a handler.
    fn deploy<'a>(
        self,
        ctx: Self::Context<'a>,
    ) -> impl Future<Output = Result<Self::Handler>> + Send + 'a
    where
        Self: 'a;
}
