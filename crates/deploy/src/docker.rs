//! Docker client for managing containers.

use std::{collections::HashMap, collections::HashSet, mem, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use bollard::{
    Docker,
    container::{
        Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, StartContainerOptions,
        StopContainerOptions, WaitContainerOptions,
    },
    image::CreateImageOptions,
    network::CreateNetworkOptions,
    secret::{HostConfig, PortBinding},
};
use derive_more::Deref;
use futures::{StreamExt, executor::block_on, future::join_all};
use tokio::{task::JoinHandle, time::timeout};
use url::Url;

/// Timeout for shutting down docker and cleaning up containers.
const DOCKER_DROP_TIMEOUT: Duration = Duration::from_secs(60);

/// Protocol for port mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortProtocol {
    #[default]
    Tcp,
    Udp,
}

impl PortProtocol {
    fn as_str(&self) -> &'static str {
        match self {
            PortProtocol::Tcp => "tcp",
            PortProtocol::Udp => "udp",
        }
    }
}

/// A port mapping from container port to host port.
#[derive(Debug, Clone)]
pub struct PortMapping {
    /// The port inside the container.
    pub container_port: u16,
    /// The port on the host.
    pub host_port: u16,
    /// The protocol (tcp or udp).
    pub protocol: PortProtocol,
}

impl PortMapping {
    /// Create a new TCP port mapping.
    pub fn tcp(container_port: u16, host_port: u16) -> Self {
        Self {
            container_port,
            host_port,
            protocol: PortProtocol::Tcp,
        }
    }

    /// Create a new UDP port mapping.
    pub fn udp(container_port: u16, host_port: u16) -> Self {
        Self {
            container_port,
            host_port,
            protocol: PortProtocol::Udp,
        }
    }

    /// Create a TCP port mapping where container and host ports are the same.
    pub fn tcp_same(port: u16) -> Self {
        Self::tcp(port, port)
    }

    /// Create a UDP port mapping where container and host ports are the same.
    pub fn udp_same(port: u16) -> Self {
        Self::udp(port, port)
    }
}

/// Configuration for starting a service container.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// The Docker image to use.
    pub image: DockerImage,
    /// The entrypoint for the container.
    pub entrypoint: Option<Vec<String>>,
    /// The command to run in the container.
    pub cmd: Option<Vec<String>>,
    /// Port mappings from container to host.
    pub port_mappings: Vec<PortMapping>,
    /// Volume binds (host:container:mode format).
    pub binds: Vec<String>,
    /// Environment variables.
    pub env: Option<Vec<String>>,
}

impl ServiceConfig {
    /// Create a new service config with the given image.
    pub fn new(image: DockerImage) -> Self {
        Self {
            image,
            entrypoint: None,
            cmd: None,
            port_mappings: Vec::new(),
            binds: Vec::new(),
            env: None,
        }
    }

    /// Set the entrypoint.
    pub fn entrypoint(mut self, entrypoint: Vec<String>) -> Self {
        self.entrypoint = Some(entrypoint);
        self
    }

    /// Set the command.
    pub fn cmd(mut self, cmd: Vec<String>) -> Self {
        self.cmd = Some(cmd);
        self
    }

    /// Add a port mapping.
    pub fn port(mut self, mapping: PortMapping) -> Self {
        self.port_mappings.push(mapping);
        self
    }

    /// Add multiple port mappings.
    pub fn ports(mut self, mappings: impl IntoIterator<Item = PortMapping>) -> Self {
        self.port_mappings.extend(mappings);
        self
    }

    /// Add a volume bind.
    pub fn bind(mut self, host_path: &PathBuf, container_path: &PathBuf, mode: &str) -> Self {
        self.binds.push(format!(
            "{}:{}:{}",
            host_path.display(),
            container_path.display(),
            mode
        ));
        self
    }

    /// Add a volume bind from a string.
    pub fn bind_str(mut self, bind: impl Into<String>) -> Self {
        self.binds.push(bind.into());
        self
    }

    /// Set environment variables.
    pub fn env(mut self, env: Vec<String>) -> Self {
        self.env = Some(env);
        self
    }
}

/// Handler returned after starting a service.
#[derive(Debug, Clone)]
pub struct ServiceHandler {
    /// The container ID.
    pub container_id: String,
    /// The container name.
    pub container_name: String,
}

/// A Docker image reference with image name and tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DockerImage {
    /// The image name (e.g., "ghcr.io/foundry-rs/foundry").
    pub image: String,
    /// The image tag (e.g., "latest" or "v1.0.0").
    pub tag: String,
}

impl DockerImage {
    /// Create a new DockerImage with the given image name and tag.
    pub fn new(image: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            tag: tag.into(),
        }
    }

    /// Pull the image, ensuring it is available locally.
    ///
    /// This will check if the image exists locally and pull it if necessary.
    pub async fn pull(&self, docker: &KupDocker) -> Result<&Self> {
        docker.pull_image(&self.image, &self.tag).await?;
        Ok(self)
    }

    /// Get the full image reference (image:tag).
    pub fn full_name(&self) -> String {
        format!("{}:{}", self.image, self.tag)
    }
}

impl std::fmt::Display for DockerImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.image, self.tag)
    }
}

/// Configuration for the Docker client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KupDockerConfig {
    /// The name of the Docker network to create.
    pub net_name: String,
    /// Whether to skip cleanup of containers on exit.
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
    pub network_id: String,

    pub config: KupDockerConfig,
}

impl Drop for KupDocker {
    fn drop(&mut self) {
        if self.config.no_cleanup {
            tracing::debug!("Cleanup of docker containers on exit is disabled. Exiting.");
            return;
        }

        if self.containers.is_empty() {
            tracing::debug!("No containers or networks to cleanup. Exiting.");
            return;
        }

        tracing::debug!("Cleaning up {} container(s)...", self.containers.len());

        // Spawn a blocking task to stop all containers
        let docker = self.docker.clone();
        let containers = mem::take(&mut self.containers);

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
            tracing::trace!(self.network_id, "Removing network");
            docker
                .remove_network(&self.network_id)
                .await
                .context("Failed to remove network")?;
            tracing::trace!(self.network_id, "Network removed");

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
    const STOP_CONTAINER_TIMEOUT: Duration = Duration::from_secs(5);

    pub async fn pull_image(&self, image: &str, tag: &str) -> Result<()> {
        let full_image = format!("{}:{}", image, tag);

        // Check if image is already available locally
        if self.docker.inspect_image(&full_image).await.is_ok() {
            tracing::debug!(image = %full_image, "Image already available locally, skipping pull");
            return Ok(());
        }

        tracing::debug!(image = %full_image, "Image not found locally, pulling...");

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
                .map_err(|e| anyhow::anyhow!("Failed to pull image '{}:{}': {}", image, tag, e))?
                .status
        {
            tracing::trace!(status, "Image pull");
        }

        Ok(())
    }

    /// Create a new Docker client.
    pub async fn new(config: KupDockerConfig) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker. Is Docker running?")?;

        let network_id = Self::create_network(&docker, &config.net_name).await?;

        Ok(Self {
            docker,
            config,
            network_id,
            containers: HashSet::new(),
        })
    }

    /// Create a Docker network for container communication.
    pub async fn create_network(docker: &Docker, network_name: &str) -> Result<String> {
        tracing::info!("Creating Docker network: {}", network_name);

        let create_network_options = CreateNetworkOptions {
            name: network_name.to_string(),
            check_duplicate: true,
            driver: "bridge".to_string(),
            ..Default::default()
        };

        let response = docker
            .create_network(create_network_options)
            .await
            .context("Failed to create Docker network")?;

        // Use the network ID from the response, or fall back to the network name
        let network_id = (!response.id.is_empty())
            .then(|| response.id)
            .unwrap_or(network_name.to_string());

        tracing::trace!(network_id, "Docker network created");

        Ok(network_id)
    }

    /// Wait for a container to complete and return its exit code.
    ///
    /// This method blocks until the container exits and returns the exit code.
    /// If the container exits with a non-zero code, an error is returned.
    pub async fn wait_for_container(&self, container_id: &str) -> Result<i64> {
        tracing::trace!(container_id, "Waiting for container to complete");

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

        tracing::debug!(container_id, exit_code, "Container completed");

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
    pub async fn stream_logs(&self, container_id: &str) -> Result<JoinHandle<()>> {
        let logs_options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            follow: true,
            ..Default::default()
        };

        let mut log_stream = self.logs(container_id, Some(logs_options));
        let container_id = container_id.to_string();

        let logs_handle = tokio::spawn(async move {
            while let Some(log_result) = log_stream.next().await {
                match log_result {
                    Ok(log) => {
                        tracing::debug!(?container_id, ?log);
                    }
                    Err(e) => {
                        tracing::error!("Error streaming logs: {}", e);
                        break;
                    }
                }
            }

            tracing::trace!(container_id, "Logs stream ended");
        });

        Ok(logs_handle)
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
        options: CreateAndStartContainerOptions,
    ) -> Result<String> {
        tracing::trace!(container_name, "Creating container");
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
        tracing::trace!(container_id, container_name, "Starting container");

        self.docker
            .start_container(&container_id, options.start_options)
            .await
            .context("Failed to start container")?;

        if options.stream_logs {
            self.stream_logs(&container_id).await?;
        }

        if options.wait_for_container {
            self.wait_for_container(&container_id).await?;
        }

        self.containers.insert(container_id.to_string());

        Ok(container_id)
    }

    async fn stop_and_remove_container_static(
        docker: &Docker,
        container_id: &String,
    ) -> Result<()> {
        tracing::trace!(container_id, "Stopping and removing container");

        // Stop the container
        docker
            .stop_container(
                container_id,
                Some(StopContainerOptions {
                    t: Self::STOP_CONTAINER_TIMEOUT.as_secs() as i64,
                }),
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

        tracing::trace!(container_id, "Container stopped and removed");
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

    /// Start a service container with the given configuration.
    ///
    /// This method handles:
    /// - Building port bindings from the port mappings
    /// - Creating the host config with network mode and binds
    /// - Creating and starting the container
    pub async fn start_service(
        &mut self,
        container_name: &str,
        config: ServiceConfig,
        options: CreateAndStartContainerOptions,
    ) -> Result<ServiceHandler> {
        // Build port bindings from the port mappings
        let port_bindings: HashMap<String, Option<Vec<PortBinding>>> = config
            .port_mappings
            .iter()
            .map(|pm| {
                (
                    format!("{}/{}", pm.container_port, pm.protocol.as_str()),
                    Some(vec![PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        host_port: Some(pm.host_port.to_string()),
                    }]),
                )
            })
            .collect();

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds: if config.binds.is_empty() {
                None
            } else {
                Some(config.binds)
            },
            network_mode: Some(self.network_id.clone()),
            ..Default::default()
        };

        let container_config = Config {
            image: Some(config.image.full_name()),
            entrypoint: config.entrypoint,
            cmd: config.cmd,
            env: config.env,
            host_config: Some(host_config),
            ..Default::default()
        };

        let container_id = self
            .create_and_start_container(container_name, container_config, options)
            .await?;

        Ok(ServiceHandler {
            container_id,
            container_name: container_name.to_string(),
        })
    }

    /// Build an HTTP RPC URL for a container.
    ///
    /// The URL uses the container name as the hostname (for Docker network communication).
    pub fn build_http_url(container_name: &str, port: u16) -> Result<Url> {
        Url::parse(&format!("http://{}:{}/", container_name, port))
            .context("Failed to parse HTTP URL")
    }

    /// Build a WebSocket RPC URL for a container.
    pub fn build_ws_url(container_name: &str, port: u16) -> Result<Url> {
        Url::parse(&format!("ws://{}:{}/", container_name, port))
            .context("Failed to parse WebSocket URL")
    }
}

pub struct CreateAndStartContainerOptions {
    pub start_options: Option<StartContainerOptions<String>>,
    pub wait_for_container: bool,
    pub stream_logs: bool,
}

impl Default for CreateAndStartContainerOptions {
    fn default() -> Self {
        Self {
            start_options: None,
            wait_for_container: false,
            stream_logs: false,
        }
    }
}
