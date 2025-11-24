//! Docker client for managing containers.

use std::{collections::HashSet, mem, time::Duration};

use anyhow::{Context, Result};
use bollard::{
    Docker,
    container::{
        Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
        WaitContainerOptions,
    },
    image::CreateImageOptions,
    network::CreateNetworkOptions,
};
use derive_more::Deref;
use futures::{StreamExt, executor::block_on, future::join_all};
use tokio::time::timeout;

/// Timeout for shutting down docker and cleaning up containers.
const DOCKER_DROP_TIMEOUT: Duration = Duration::from_secs(60);

pub struct KupDockerConfig {
    pub foundry_docker_image: String,
    pub foundry_docker_tag: String,

    pub op_deployer_docker_image: String,
    pub op_deployer_docker_tag: String,

    pub net_name: String,

    pub no_cleanup: bool,
}

/// Docker client wrapper for Foundry operations.
#[derive(Deref)]
pub struct KupDocker {
    #[deref]
    docker: Docker,

    /// Containers that have been started.
    pub containers: HashSet<String>,

    /// Network ID for container communication.
    pub network_id: Option<String>,

    pub config: KupDockerConfig,
}

impl Drop for KupDocker {
    fn drop(&mut self) {
        if self.config.no_cleanup {
            tracing::info!("Cleanup of docker containers on exit is disabled. Exiting.");
            return;
        }

        if self.containers.is_empty() && self.network_id.is_none() {
            tracing::info!("No containers or networks to cleanup. Exiting.");
            return;
        }

        tracing::info!("Cleaning up {} container(s)...", self.containers.len());

        // Spawn a blocking task to stop all containers
        let docker = self.docker.clone();
        let containers = mem::take(&mut self.containers);
        let network_id = self.network_id.take();

        let cleanup = async {
            // Stop and remove containers first
            let results = containers
                .into_iter()
                .map(async |container_id| {
                    Self::stop_and_remove_container_static(&docker, &container_id).await
                })
                .collect::<Vec<_>>();

            timeout(DOCKER_DROP_TIMEOUT, join_all(results))
                .await?
                .into_iter()
                .collect::<Result<Vec<_>>>()?;

            // Remove network if it exists
            if let Some(network_id) = network_id {
                tracing::info!("Removing network: {}", network_id);
                docker
                    .remove_network(&network_id)
                    .await
                    .context("Failed to remove network")?;
                tracing::info!("✓ Network removed successfully");
            }

            Ok::<_, anyhow::Error>(())
        };

        if let Err(e) = block_on(cleanup) {
            tracing::error!(error = ?e, "Failed to cleanup containers and networks");
            return;
        }

        tracing::info!("✓ Cleanup completed successfully");
    }
}

impl KupDocker {
    pub async fn pull_image(&self, image: &str, tag: &str) -> Result<()> {
        let mut stream = self.docker.create_image(
            Some(CreateImageOptions {
                from_image: image.to_string(),
                tag: tag.to_string(),
                ..Default::default()
            }),
            None,
            None,
        );

        while let Some(result) = stream.next().await
            && let Some(status) = result
                .map_err(|e| anyhow::anyhow!("Failed to pull Foundry image: {}", e))?
                .status
        {
            tracing::debug!("Image pull: {}", status);
        }

        Ok(())
    }

    /// Create a new Docker client.
    pub async fn new(config: KupDockerConfig) -> Result<Self> {
        let mut docker = Self {
            docker: Docker::connect_with_local_defaults()
                .context("Failed to connect to Docker. Is Docker running?")?,
            config,
            containers: HashSet::new(),
            network_id: None,
        };

        let network_name = docker.config.net_name.clone();

        // Create a Docker network for container communication
        docker
            .create_network(&network_name)
            .await
            .context("Failed to create Docker network")?;

        tracing::info!("✓ Docker network created: {}", network_name);

        tracing::info!(
            image = docker.config.foundry_docker_image,
            tag = docker.config.foundry_docker_tag,
            "Pulling Foundry from docker..."
        );

        docker
            .pull_image(
                &docker.config.foundry_docker_image,
                &docker.config.foundry_docker_tag,
            )
            .await?;

        tracing::info!(
            image = docker.config.op_deployer_docker_image,
            tag = docker.config.op_deployer_docker_tag,
            "Pulling Op Deployer from docker..."
        );

        docker
            .pull_image(
                &docker.config.op_deployer_docker_image,
                &docker.config.op_deployer_docker_tag,
            )
            .await?;

        tracing::info!("✓ Images pulled successfully");

        Ok(docker)
    }

    /// Create a Docker network for container communication.
    pub async fn create_network(&mut self, network_name: &str) -> Result<String> {
        tracing::info!("Creating Docker network: {}", network_name);

        let create_network_options = CreateNetworkOptions {
            name: network_name.to_string(),
            check_duplicate: true,
            driver: "bridge".to_string(),
            ..Default::default()
        };

        let response = self
            .docker
            .create_network(create_network_options)
            .await
            .context("Failed to create Docker network")?;

        // Use the network ID from the response, or fall back to the network name
        let network_id = (!response.id.is_empty())
            .then(|| response.id)
            .unwrap_or(network_name.to_string());

        self.network_id = Some(network_id.clone());
        tracing::info!("✓ Docker network created: {}", network_id);

        Ok(network_id)
    }

    /// Wait for a container to complete and return its exit code.
    ///
    /// This method blocks until the container exits and returns the exit code.
    /// If the container exits with a non-zero code, an error is returned.
    pub async fn wait_for_container(&self, container_id: &str) -> Result<i64> {
        tracing::info!("Waiting for container {} to complete...", container_id);

        let wait_options = WaitContainerOptions {
            condition: "not-running",
        };

        let mut wait_stream = self.docker.wait_container(container_id, Some(wait_options));

        // Wait for the container to exit
        let exit_code = if let Some(wait_result) = wait_stream.next().await {
            let response = wait_result.context("Failed to wait for container")?;
            response.status_code
        } else {
            anyhow::bail!("Container wait stream ended without response");
        };

        tracing::info!(
            "Container {} completed with exit code: {}",
            container_id,
            exit_code
        );

        if exit_code != 0 {
            anyhow::bail!(
                "Container {} exited with non-zero code: {}",
                container_id,
                exit_code
            );
        }

        Ok(exit_code)
    }

    /// Stream logs from a container.
    pub async fn stream_logs(&self, container_id: &str) -> Result<()> {
        let logs_options = LogsOptions::<String> {
            follow: true,
            stdout: true,
            stderr: true,
            ..Default::default()
        };

        let mut log_stream = self.logs(container_id, Some(logs_options));

        while let Some(log_result) = log_stream.next().await {
            match log_result {
                Ok(log) => {
                    tracing::debug!("[Container {}] {}", container_id, log);
                }
                Err(e) => {
                    tracing::error!("Error streaming logs: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Stream logs and wait for container completion simultaneously.
    ///
    /// This is useful for short-lived containers where you want to see the output
    /// and know when they're done.
    pub async fn stream_logs_and_wait(&self, container_id: &str) -> Result<i64> {
        let logs_future = self.stream_logs(container_id);
        let wait_future = self.wait_for_container(container_id);

        // Run both futures concurrently
        let (logs_result, wait_result) = tokio::join!(logs_future, wait_future);

        // Check if logging had any errors (but don't fail on them)
        if let Err(e) = logs_result {
            tracing::warn!("Error streaming logs: {}", e);
        }

        // Return the wait result (which includes the exit code)
        wait_result
    }

    /// Create and start a container.
    pub async fn create_and_start_container(
        &mut self,
        container_name: &str,
        config: Config<String>,
        options: Option<StartContainerOptions<String>>,
    ) -> Result<String> {
        tracing::info!("Creating container: {}", container_name);
        // Create the container
        let container = self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name,
                    ..Default::default()
                }),
                config,
            )
            .await
            .context("Failed to create container")?;

        let container_id = container.id;
        tracing::info!("Starting container: {}", container_id);

        self.docker
            .start_container(&container_id, options)
            .await
            .context("Failed to start container")?;

        self.containers.insert(container_id.to_string());

        Ok(container_id)
    }

    async fn stop_and_remove_container_static(
        docker: &Docker,
        container_id: &String,
    ) -> Result<()> {
        tracing::info!("Stopping container: {}", container_id);

        // Kill the container (stop with timeout=0)
        docker
            .kill_container(
                container_id,
                None::<bollard::container::KillContainerOptions<String>>,
            )
            .await
            .ok(); // Ignore errors if already stopped

        // Remove the container
        docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .ok(); // Ignore errors if already removed

        tracing::info!("✓ Container stopped: {}", container_id);
        Ok(())
    }

    /// Stop and remove a container.
    pub async fn stop_and_remove_container(&self, container_id: &String) -> Result<()> {
        Self::stop_and_remove_container_static(&self.docker, container_id).await
    }

    /// Check if a container is running.
    pub async fn is_container_running(&self, container_name: &str) -> Result<bool> {
        match self.inspect_container(container_name, None).await {
            Ok(info) => {
                if let Some(state) = info.state {
                    return Ok(state.running.unwrap_or(false));
                }
                Ok(false)
            }
            Err(_) => Ok(false),
        }
    }
}
