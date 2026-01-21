//! Docker client for managing containers.

use std::{
    collections::HashMap,
    collections::HashSet,
    fs,
    io::Read as _,
    mem,
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};

use anyhow::{Context, Result};
use bollard::{
    Docker,
    container::{
        Config, CreateContainerOptions, ListContainersOptions, LogsOptions, RemoveContainerOptions,
        StartContainerOptions, StopContainerOptions, WaitContainerOptions,
    },
    image::{BuildImageOptions, CreateImageOptions},
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
    pub fn display_container_with_protocol(&self) -> String {
        format!("{}/{}", self.container_port, self.protocol.as_str())
    }

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

    /// Create an optional TCP port mapping.
    /// If `host_port` is `Some(port)`, creates a mapping to that port.
    /// If `host_port` is `None`, returns `None` (port not published).
    pub fn tcp_optional(container_port: u16, host_port: Option<u16>) -> Option<Self> {
        host_port.map(|hp| Self::tcp(container_port, hp))
    }

    /// Create an optional UDP port mapping.
    /// If `host_port` is `Some(port)`, creates a mapping to that port.
    /// If `host_port` is `None`, returns `None` (port not published).
    pub fn udp_optional(container_port: u16, host_port: Option<u16>) -> Option<Self> {
        host_port.map(|hp| Self::udp(container_port, hp))
    }
}

/// An exposed port within the Docker network (container-to-container).
#[derive(Debug, Clone)]
pub struct ExposedPort {
    /// The port inside the container.
    pub port: u16,
    /// The protocol (tcp or udp).
    pub protocol: PortProtocol,
}

impl ExposedPort {
    /// Create a new TCP exposed port.
    pub fn tcp(port: u16) -> Self {
        Self {
            port,
            protocol: PortProtocol::Tcp,
        }
    }

    /// Create a new UDP exposed port.
    pub fn udp(port: u16) -> Self {
        Self {
            port,
            protocol: PortProtocol::Udp,
        }
    }

    fn display_with_protocol(&self) -> String {
        format!("{}/{}", self.port, self.protocol.as_str())
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
    /// Ports exposed within the Docker network (for container-to-container communication).
    /// These ports are accessible by other containers on the same network.
    pub exposed_ports: Vec<ExposedPort>,
    /// Port bindings from container to host (published to host machine).
    /// These ports are accessible from the host machine.
    pub port_bindings: Vec<PortMapping>,
    /// Volume binds (host:container:mode format).
    pub binds: Vec<String>,
    /// Environment variables.
    pub env: Option<Vec<String>>,
    /// User to run the container as (e.g., "1000:1000" for UID:GID).
    pub user: Option<String>,
}

impl ServiceConfig {
    /// Create a new service config with the given image.
    pub fn new(image: DockerImage) -> Self {
        Self {
            image,
            entrypoint: None,
            cmd: None,
            exposed_ports: Vec::new(),
            port_bindings: Vec::new(),
            binds: Vec::new(),
            env: None,
            user: None,
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

    /// Add a port to expose within the Docker network (container-to-container).
    pub fn expose(mut self, port: ExposedPort) -> Self {
        self.exposed_ports.push(port);
        self
    }

    /// Add multiple ports to expose within the Docker network.
    pub fn expose_ports(mut self, ports: impl IntoIterator<Item = ExposedPort>) -> Self {
        self.exposed_ports.extend(ports);
        self
    }

    /// Add a port binding (publish to host).
    pub fn port(mut self, mapping: PortMapping) -> Self {
        self.port_bindings.push(mapping);
        self
    }

    /// Add multiple port bindings (publish to host).
    pub fn ports(mut self, mappings: impl IntoIterator<Item = PortMapping>) -> Self {
        self.port_bindings.extend(mappings);
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

    /// Set the user to run the container as (e.g., "1000:1000" for UID:GID).
    pub fn user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
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
    /// Map of container port to actual bound host port.
    /// Key format: "port/protocol" (e.g., "8545/tcp")
    pub bound_ports: HashMap<String, u16>,
}

impl ServiceHandler {
    /// Get the bound host port for a container port.
    /// Returns None if the port is not published to the host.
    pub fn get_host_port(&self, container_port: u16, protocol: &str) -> Option<u16> {
        let key = format!("{}/{}", container_port, protocol);
        self.bound_ports.get(&key).copied()
    }

    /// Get the bound host port for a TCP container port.
    pub fn get_tcp_host_port(&self, container_port: u16) -> Option<u16> {
        self.get_host_port(container_port, "tcp")
    }
}

/// A Docker image reference with image name and tag.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DockerImage {
    /// The image name (e.g., "ghcr.io/foundry-rs/foundry").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// The image tag (e.g., "latest" or "v1.0.0").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// Path to local binary (takes precedence over image/tag).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<PathBuf>,
}

impl DockerImage {
    /// Create a new DockerImage with the given image name and tag.
    pub fn new(image: impl Into<String>, tag: impl Into<String>) -> Self {
        Self {
            image: Some(image.into()),
            tag: Some(tag.into()),
            binary: None,
        }
    }

    /// Create a DockerImage from a local binary path.
    pub fn from_binary(path: impl Into<PathBuf>) -> Self {
        Self {
            image: None,
            tag: None,
            binary: Some(path.into()),
        }
    }

    /// Returns true if this image uses a local binary.
    pub fn is_local_binary(&self) -> bool {
        self.binary.is_some()
    }

    /// Get the binary path if set.
    pub fn binary_path(&self) -> Option<&Path> {
        self.binary.as_deref()
    }

    /// Get the image reference string (image:tag).
    /// Panics if called on a local binary image (use ensure_image_ready instead).
    pub fn image_ref(&self) -> String {
        match (&self.image, &self.tag) {
            (Some(image), Some(tag)) => format!("{}:{}", image, tag),
            _ => panic!("image_ref() called on local binary DockerImage"),
        }
    }

    /// Pull the image, ensuring it is available locally.
    ///
    /// This will check if the image exists locally and pull it if necessary.
    /// For local binaries, use KupDocker::ensure_image_ready() instead.
    pub async fn pull(&self, docker: &KupDocker) -> Result<String> {
        if self.is_local_binary() {
            anyhow::bail!("Cannot pull a local binary image. Use ensure_image_ready() instead.");
        }
        let image = self.image.as_ref().context("Missing image name")?;
        let tag = self.tag.as_ref().context("Missing image tag")?;
        docker.pull_image(image, tag).await
    }
}

impl std::fmt::Display for DockerImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(binary) = &self.binary {
            write!(f, "local:{}", binary.display())
        } else if let (Some(image), Some(tag)) = (&self.image, &self.tag) {
            write!(f, "{}:{}", image, tag)
        } else {
            write!(f, "<invalid>")
        }
    }
}

/// Configuration for the Docker client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KupDockerConfig {
    /// The name of the Docker network to create.
    pub net_name: String,
    /// Whether to skip cleanup of containers on exit.
    pub no_cleanup: bool,
    /// Whether to publish all exposed ports to random host ports.
    #[serde(default)]
    pub publish_all_ports: bool,
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

pub struct CreateAndStartContainerResult {
    pub container_id: String,
    pub logs: String,
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

    pub async fn pull_image(&self, image: &str, tag: &str) -> Result<String> {
        let full_image = format!("{}:{}", image, tag);

        // Check if image is already available locally
        if self.docker.inspect_image(&full_image).await.is_ok() {
            tracing::debug!(image = %full_image, "Image already available locally, skipping pull");
            return Ok(full_image);
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

        Ok(full_image)
    }

    /// Build a Docker image from a local binary file.
    ///
    /// This creates a lightweight image based on `debian:bookworm-slim` with the binary
    /// copied to `/binary` and set as the entrypoint.
    ///
    /// The image is tagged as `kupcake-{service_name}-local:{hash}` where hash is the
    /// first 12 characters of the SHA256 hash of the binary content. This enables
    /// caching: if the image already exists with this tag, the build is skipped.
    ///
    /// # Arguments
    /// * `binary_path` - Path to the local binary file
    /// * `service_name` - Name of the service (used in the image tag)
    ///
    /// # Returns
    /// The full image reference (e.g., `kupcake-op-reth-local:a1b2c3d4e5f6`)
    pub async fn build_local_image(
        &self,
        binary_path: &Path,
        service_name: &str,
    ) -> Result<String> {
        // Validate binary exists and is readable
        let binary_path = binary_path
            .canonicalize()
            .with_context(|| format!("Binary not found: {}", binary_path.display()))?;

        let metadata = fs::metadata(&binary_path)
            .with_context(|| format!("Cannot read binary metadata: {}", binary_path.display()))?;

        if !metadata.is_file() {
            anyhow::bail!("Path is not a file: {}", binary_path.display());
        }

        // Compute SHA256 hash of the binary
        let hash = Self::compute_file_hash(&binary_path)
            .with_context(|| format!("Failed to hash binary: {}", binary_path.display()))?;

        // Use first 12 characters of hex hash for tag
        let short_hash = &hash[..12];
        let image_name = format!("kupcake-{}-local", service_name);
        let image_ref = format!("{}:{}", image_name, short_hash);

        // Check if image already exists (skip build if so)
        if self.docker.inspect_image(&image_ref).await.is_ok() {
            tracing::debug!(
                image = %image_ref,
                binary = %binary_path.display(),
                "Local image already exists, skipping build"
            );
            return Ok(image_ref);
        }

        tracing::info!(
            service = %service_name,
            binary = %binary_path.display(),
            image = %image_ref,
            "Building local image from binary"
        );

        // Pull base image if needed (using trixie for GLIBC 2.38+ support)
        self.pull_image("debian", "trixie-slim").await?;

        // Create tar archive with Dockerfile and binary
        let tar_bytes = Self::create_build_context(&binary_path)
            .with_context(|| format!("Failed to create build context for {}", binary_path.display()))?;

        // Build the image
        let build_options = BuildImageOptions {
            dockerfile: "Dockerfile".to_string(),
            t: image_ref.clone(),
            rm: true,
            forcerm: true,
            ..Default::default()
        };

        let mut build_stream = self.docker.build_image(
            build_options,
            None,
            Some(tar_bytes.into()),
        );

        // Process build stream and check for errors
        while let Some(result) = build_stream.next().await {
            let build_info = result
                .map_err(|e| anyhow::anyhow!("Docker build error: {}", e))?;

            // Log build progress
            if let Some(stream) = &build_info.stream {
                let stream = stream.trim();
                if !stream.is_empty() {
                    tracing::trace!(stream, "Docker build");
                }
            }

            // Check for build errors
            if let Some(error) = &build_info.error {
                anyhow::bail!("Docker build failed: {}", error);
            }
        }

        tracing::info!(
            image = %image_ref,
            "Local image built successfully"
        );

        Ok(image_ref)
    }

    /// Compute SHA256 hash of a file, returning the hex-encoded string.
    fn compute_file_hash(path: &Path) -> Result<String> {
        let mut file = fs::File::open(path)
            .with_context(|| format!("Failed to open file: {}", path.display()))?;

        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];

        loop {
            let bytes_read = file.read(&mut buffer)
                .with_context(|| format!("Failed to read file: {}", path.display()))?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        let hash = hasher.finalize();
        Ok(hex::encode(hash))
    }

    /// Create a tar archive containing the Dockerfile and binary for building.
    fn create_build_context(binary_path: &Path) -> Result<Vec<u8>> {
        use std::io::Cursor;

        let binary_data = fs::read(binary_path)
            .with_context(|| format!("Failed to read binary: {}", binary_path.display()))?;

        let dockerfile_content = b"FROM debian:trixie-slim
COPY binary /binary
RUN chmod +x /binary
ENTRYPOINT [\"/binary\"]
";

        // Create tar archive in memory
        let mut tar_buffer = Vec::new();
        {
            let cursor = Cursor::new(&mut tar_buffer);
            let mut tar_builder = tar::Builder::new(cursor);

            // Add Dockerfile
            let mut dockerfile_header = tar::Header::new_gnu();
            dockerfile_header.set_path("Dockerfile")?;
            dockerfile_header.set_size(dockerfile_content.len() as u64);
            dockerfile_header.set_mode(0o644);
            dockerfile_header.set_cksum();
            tar_builder.append(&dockerfile_header, &dockerfile_content[..])?;

            // Add binary
            let mut binary_header = tar::Header::new_gnu();
            binary_header.set_path("binary")?;
            binary_header.set_size(binary_data.len() as u64);
            binary_header.set_mode(0o755);
            binary_header.set_cksum();
            tar_builder.append(&binary_header, binary_data.as_slice())?;

            tar_builder.finish()?;
        }

        Ok(tar_buffer)
    }

    /// Ensure a DockerImage is ready for use.
    ///
    /// For remote images: pulls the image if not available locally.
    /// For local binaries: builds the image from the binary if not already cached.
    ///
    /// # Arguments
    /// * `docker_image` - The DockerImage configuration
    /// * `service_name` - Name of the service (used for local binary image tags)
    ///
    /// # Returns
    /// The image reference string to use when creating containers.
    pub async fn ensure_image_ready(
        &self,
        docker_image: &DockerImage,
        service_name: &str,
    ) -> Result<String> {
        if let Some(binary_path) = docker_image.binary_path() {
            self.build_local_image(binary_path, service_name).await
        } else {
            docker_image.pull(self).await
        }
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
    ///
    /// If the network already exists, this function will use the existing network
    /// instead of failing.
    pub async fn create_network(docker: &Docker, network_name: &str) -> Result<String> {
        tracing::info!("Creating Docker network: {}", network_name);

        // First, check if the network already exists
        match docker.inspect_network::<String>(network_name, None).await {
            Ok(network_info) => {
                let network_id = network_info.id.unwrap_or_else(|| network_name.to_string());
                tracing::info!(
                    network_id = %network_id,
                    network_name = %network_name,
                    "Docker network already exists, reusing it"
                );
                return Ok(network_id);
            }
            Err(_) => {
                // Network doesn't exist, create it
            }
        }

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
            .then_some(response.id)
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
            let response = wait_result.map_err(|e| {
                tracing::error!(container_id, error = ?e, "Docker wait_container error");
                anyhow::anyhow!("Docker container wait error: {}", e)
            })?;
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
    ) -> Result<CreateAndStartContainerResult> {
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

        let logs = if options.collect_logs {
            self.collect_container_logs(&container_id, true, true).await
        } else {
            String::new()
        };

        self.containers.insert(container_id.to_string());

        Ok(CreateAndStartContainerResult { container_id, logs })
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

    /// Get the bound host ports for a running container.
    ///
    /// Returns a map of "container_port/protocol" -> actual_host_port.
    pub async fn get_container_bound_ports(
        &self,
        container_id: &str,
    ) -> Result<HashMap<String, u16>> {
        let inspect = self
            .docker
            .inspect_container(container_id, None)
            .await
            .context("Failed to inspect container for port bindings")?;

        // This closure parses the container bindings and returns a map of container port to host port.
        let parse_bindings = |container_port: String, bindings: Vec<PortBinding>| {
            bindings.into_iter().filter_map(move |binding| {
                binding.host_port.and_then(|host_port| {
                    // Skip empty strings (these are unbound ports)
                    if host_port.is_empty() {
                        return None;
                    }
                    let host_port = host_port.parse::<u16>().ok()?;
                    Some((container_port.to_string(), host_port))
                })
            })
        };

        // This closure inspects the container ports and returns a map of container port to host port.
        let inspect_ports = |ports: HashMap<String, Option<Vec<PortBinding>>>| {
            ports
                .into_iter()
                .filter_map(|(container_port, maybe_bindings)| {
                    maybe_bindings.map(|bindings| parse_bindings(container_port, bindings))
                })
                .flatten()
                .collect::<HashMap<String, u16>>()
        };

        let bound_ports = inspect
            .network_settings
            .and_then(|network_settings| network_settings.ports)
            .map(inspect_ports)
            .unwrap_or_default();

        tracing::debug!(?bound_ports, "Container bound ports");

        Ok(bound_ports)
    }

    /// Build Docker container configuration from a ServiceConfig.
    fn build_container_config(
        &self,
        config: ServiceConfig,
        image: String,
        options: ContainerConfigOptions,
    ) -> Config<String> {
        // Build port bindings from the port_bindings (ports published to host)
        // When host_port is 0, we pass an empty string to let Docker assign a random available port
        let port_bindings: HashMap<String, Option<Vec<PortBinding>>> = config
            .port_bindings
            .iter()
            .map(|pm| {
                (
                    pm.display_container_with_protocol(),
                    Some(vec![PortBinding {
                        host_ip: Some("0.0.0.0".to_string()),
                        // Empty string tells Docker to assign a random port
                        host_port: if pm.host_port == 0 {
                            Some(String::new())
                        } else {
                            Some(pm.host_port.to_string())
                        },
                    }]),
                )
            })
            .collect();

        // Build exposed ports: combine explicit exposed_ports and ports from port_bindings
        // Exposed ports are required for port bindings to work, and also allow
        // container-to-container communication on the Docker network
        let mut exposed_ports: HashMap<String, HashMap<(), ()>> = config
            .exposed_ports
            .iter()
            .map(|ep| (ep.display_with_protocol(), HashMap::new()))
            .collect();

        // Also expose ports that have bindings (required for bindings to work)
        for pm in &config.port_bindings {
            exposed_ports
                .entry(pm.display_container_with_protocol())
                .or_default();
        }

        let has_port_bindings = !port_bindings.is_empty();
        let has_exposed_ports = !exposed_ports.is_empty();

        let host_config = HostConfig {
            port_bindings: has_port_bindings.then_some(port_bindings),
            binds: (!config.binds.is_empty()).then_some(config.binds),
            network_mode: Some(self.network_id.clone()),
            auto_remove: options.auto_remove.then_some(true),
            publish_all_ports: self.config.publish_all_ports.then_some(true),
            ..Default::default()
        };

        Config {
            image: Some(image),
            entrypoint: config.entrypoint,
            cmd: config.cmd,
            env: config.env,
            user: config.user,
            exposed_ports: has_exposed_ports.then_some(exposed_ports),
            host_config: Some(host_config),
            ..Default::default()
        }
    }

    /// Start a service container with the given configuration.
    ///
    /// This method handles:
    /// - Building port bindings from the port mappings
    /// - Creating the host config with network mode and binds
    /// - Creating and starting the container
    /// - Retrieving the actual bound host ports
    pub async fn start_service(
        &mut self,
        container_name: &str,
        config: ServiceConfig,
        options: CreateAndStartContainerOptions,
    ) -> Result<ServiceHandler> {
        let image = self.ensure_image_ready(&config.image, container_name).await?;

        let container_config =
            self.build_container_config(config, image, ContainerConfigOptions::default());

        tracing::debug!(container_name, "Creating service container");

        let create_and_start_result = self
            .create_and_start_container(container_name, container_config, options)
            .await?;

        // Get the actual bound host ports after container is started
        let bound_ports = self
            .get_container_bound_ports(&create_and_start_result.container_id)
            .await?;

        tracing::debug!(
            container_name,
            ?bound_ports,
            "Container started with bound ports"
        );

        Ok(ServiceHandler {
            container_id: create_and_start_result.container_id,
            container_name: container_name.to_string(),
            bound_ports,
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

    /// Run a command in a temporary container and capture its stdout.
    ///
    /// The container is automatically removed after the command completes.
    /// Returns the stdout output as a string.
    pub async fn run_command(&mut self, config: ServiceConfig) -> Result<String> {
        // Generate a unique container name
        let container_name = format!(
            "kupcake-cmd-{}",
            names::Generator::default().next().unwrap_or_default()
        );

        let image = self.ensure_image_ready(&config.image, &container_name).await?;

        let container_config = self.build_container_config(
            config,
            image,
            ContainerConfigOptions { auto_remove: false },
        );

        tracing::trace!(container_name, "Running command in container");

        // Create and start the container, then wait for it to complete
        let create_and_start_result = self
            .create_and_start_container(
                &container_name,
                container_config,
                CreateAndStartContainerOptions {
                    stream_logs: true,
                    wait_for_container: true,
                    start_options: None,
                    collect_logs: true,
                },
            )
            .await
            .context("Failed to run command container")?;

        // Check exit code and collect stderr if failed
        Ok(create_and_start_result.logs)
    }

    /// Collect logs from a container.
    ///
    /// Returns the collected log output as a string.
    async fn collect_container_logs(
        &self,
        container_id: &str,
        stdout: bool,
        stderr: bool,
    ) -> String {
        let logs_options = LogsOptions::<String> {
            stdout,
            stderr,
            follow: false,
            ..Default::default()
        };

        let mut log_stream = self.docker.logs(container_id, Some(logs_options));
        let mut output = String::new();

        while let Some(log_result) = log_stream.next().await {
            match log_result {
                Ok(log) => output.push_str(&log.to_string()),
                Err(e) => {
                    tracing::warn!("Error reading container logs: {}", e);
                    break;
                }
            }
        }

        output
    }
}

#[derive(Default)]
pub struct CreateAndStartContainerOptions {
    pub start_options: Option<StartContainerOptions<String>>,
    pub wait_for_container: bool,
    pub stream_logs: bool,
    pub collect_logs: bool,
}


/// Options for building a container configuration.
#[derive(Default)]
struct ContainerConfigOptions {
    /// Whether to auto-remove the container after it exits.
    auto_remove: bool,
}

/// Clean up containers and network by name prefix.
///
/// This is a standalone function that doesn't require a `KupDocker` instance.
/// It finds all containers whose names start with the given prefix, stops and removes them,
/// then removes the associated network.
pub async fn cleanup_by_prefix(prefix: &str) -> Result<CleanupResult> {
    let docker = Docker::connect_with_local_defaults()
        .context("Failed to connect to Docker daemon")?;

    let mut result = CleanupResult::default();

    // List all containers (including stopped ones) that match the prefix
    let filters: HashMap<String, Vec<String>> = HashMap::new();
    let options = ListContainersOptions {
        all: true,
        filters,
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .context("Failed to list containers")?;

    // Filter containers by name prefix
    let matching_containers: Vec<_> = containers
        .into_iter()
        .filter(|c| {
            c.names.as_ref().is_some_and(|names| {
                names.iter().any(|name| {
                    // Container names from Docker API start with "/"
                    let name = name.strip_prefix('/').unwrap_or(name);
                    name.starts_with(prefix)
                })
            })
        })
        .collect();

    if matching_containers.is_empty() {
        tracing::info!("No containers found with prefix '{}'", prefix);
    } else {
        tracing::info!(
            "Found {} container(s) with prefix '{}'",
            matching_containers.len(),
            prefix
        );

        // Stop and remove each container
        for container in matching_containers {
            let container_id = container.id.unwrap_or_default();
            let container_name = container
                .names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.strip_prefix('/').unwrap_or(n))
                .unwrap_or(&container_id)
                .to_string();

            tracing::debug!("Stopping and removing container: {}", container_name);

            // Stop the container (ignore errors if already stopped)
            docker
                .stop_container(&container_id, Some(StopContainerOptions { t: 5 }))
                .await
                .ok();

            // Remove the container
            if let Err(e) = docker
                .remove_container(
                    &container_id,
                    Some(RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await
            {
                tracing::warn!("Failed to remove container {}: {}", container_name, e);
            } else {
                result.containers_removed.push(container_name);
            }
        }
    }

    // Try to remove the network
    let network_name = format!("{}-network", prefix);
    tracing::debug!("Attempting to remove network: {}", network_name);

    match docker.remove_network(&network_name).await {
        Ok(_) => {
            result.network_removed = Some(network_name);
        }
        Err(e) => {
            // Only log if it's not a "not found" error
            let err_str = e.to_string();
            if !err_str.contains("No such network") && !err_str.contains("not found") {
                tracing::warn!("Failed to remove network {}: {}", network_name, e);
            } else {
                tracing::debug!("Network '{}' does not exist", network_name);
            }
        }
    }

    Ok(result)
}

/// Result of a cleanup operation.
#[derive(Debug, Default)]
pub struct CleanupResult {
    /// Names of containers that were removed.
    pub containers_removed: Vec<String>,
    /// Name of the network that was removed, if any.
    pub network_removed: Option<String>,
}
