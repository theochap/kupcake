//! KupcakeService trait — the common interface for all deployable services.

use std::future::Future;
use std::path::Path;

use anyhow::Result;

use crate::docker::{
    CreateAndStartContainerOptions, DockerImage, KupDocker, ServiceConfig, ServiceHandler,
};

/// Common interface for all deployable services in Kupcake.
///
/// Each service (Anvil, op-reth, kona-node, op-batcher, etc.) implements this trait,
/// including composite services like L2Node which delegate to their children.
///
/// # Type Parameters
///
/// - `Input` — per-deploy parameters that vary between invocations (URLs,
///   peer lists, etc.). Decoupled from handler types — uses owned data.
/// - `Output` — the handler returned after a successful deploy (e.g., `OpRethHandler`).
pub trait KupcakeService: Send + Sync + 'static {
    /// Per-deploy input parameters.
    type Input: Send;

    /// Handler returned after a successful deploy.
    type Output: Send;

    /// The container name for this service.
    fn container_name(&self) -> &str;

    /// The Docker image for this service.
    fn docker_image(&self) -> &DockerImage;

    /// Deploy the service: pull image, build command, start container, return handler.
    fn deploy<'a>(
        &'a self,
        docker: &'a mut KupDocker,
        host_config_path: &'a Path,
        input: Self::Input,
    ) -> impl Future<Output = Result<Self::Output>> + Send + 'a;
}

/// Common deploy pipeline for single-container services.
///
/// Ensures the image is ready, then starts the container with the given config.
/// Leaf services call this from their `deploy` implementation after building
/// their `ServiceConfig`.
pub async fn deploy_container(
    docker: &mut KupDocker,
    image: &DockerImage,
    container_name: &str,
    service_config: ServiceConfig,
) -> Result<ServiceHandler> {
    docker.ensure_image_ready(image, container_name).await?;
    docker
        .start_service(
            container_name,
            service_config,
            CreateAndStartContainerOptions::default(),
        )
        .await
}
