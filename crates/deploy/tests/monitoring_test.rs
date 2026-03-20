//! Integration tests for monitoring (Prometheus + Grafana).
//!
//! - `test_prometheus_picks_up_added_node`: Verifies that when a validator is added
//!   via node lifecycle, Prometheus is reconfigured and starts scraping the new node.
//! - `test_prometheus_tsdb_persists_across_restart`: Verifies that Prometheus TSDB data
//!   (historical metrics) survives container stop + recreate cycles.
//!
//! Run with: cargo test --test monitoring_test

mod common;

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use backon::{ConstantBuilder, Retryable};
use kupcake_deploy::{
    Deployer, DeployerBuilder, DeploymentTarget, KupDocker, KupDockerConfig, OutDataPath,
    cleanup_by_prefix, node_lifecycle, rpc,
};

use common::*;

/// Deploy a minimal network with monitoring enabled.
async fn deploy_with_monitoring(
    ctx: &TestContext,
) -> Result<(KupDocker, kupcake_deploy::DeploymentResult)> {
    let dashboards_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("grafana/dashboards"))
        .context("Failed to resolve dashboards path")?;

    let deployer = DeployerBuilder::new(ctx.l1_chain_id)
        .network_name(&ctx.network_name)
        .outdata(OutDataPath::Path(ctx.outdata_path.clone()))
        .l2_node_count(2) // 1 sequencer + 1 validator
        .sequencer_count(1)
        .block_time(2)
        .detach(true)
        .dump_state(false)
        .deployment_target(DeploymentTarget::Genesis)
        .no_proposer(true)
        .no_challenger(true)
        .monitoring_enabled(true)
        .dashboards_path(dashboards_path)
        .build()
        .await
        .context("Failed to build deployer")?;

    deployer.save_config()?;
    ctx.deploy(deployer).await
}

/// Create a KupDocker client for lifecycle operations (no cleanup on drop).
async fn lifecycle_docker(deployer: &Deployer) -> Result<KupDocker> {
    KupDocker::new(KupDockerConfig {
        no_cleanup: true,
        ..deployer.docker.clone()
    })
    .await
    .context("Failed to create Docker client for lifecycle operations")
}

/// Query Prometheus for the number of samples of a given metric over a time range.
///
/// Uses the `count_over_time` PromQL function to count how many data points
/// exist for the metric in the given window.
async fn query_prometheus_sample_count(
    prometheus_url: &str,
    metric: &str,
    range: &str,
) -> Result<u64> {
    let client = rpc::create_client()?;
    let query = format!("count_over_time({}[{}])", metric, range);
    let url = format!("{}api/v1/query?query={}", prometheus_url, query);
    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .context("Failed to query Prometheus")?
        .json()
        .await
        .context("Failed to parse Prometheus response")?;

    let results = resp["data"]["result"]
        .as_array()
        .context("No result array in Prometheus response")?;

    // Sum counts across all matching time series
    let total: u64 = results
        .iter()
        .filter_map(|r| r["value"][1].as_str())
        .filter_map(|v| v.parse::<u64>().ok())
        .sum();

    Ok(total)
}

/// Query Prometheus `/api/v1/targets` and return the list of active target job names.
async fn get_prometheus_active_targets(prometheus_url: &str) -> Result<Vec<String>> {
    let client = rpc::create_client()?;
    let url = format!("{}api/v1/targets", prometheus_url);
    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .await
        .context("Failed to query Prometheus targets")?
        .json()
        .await
        .context("Failed to parse Prometheus targets response")?;

    let targets = resp["data"]["activeTargets"]
        .as_array()
        .context("No activeTargets in Prometheus response")?;

    let job_names: Vec<String> = targets
        .iter()
        .filter_map(|t| t["labels"]["job"].as_str().map(String::from))
        .collect();

    Ok(job_names)
}

/// Wait until Prometheus has an active target whose job name contains the given substring.
async fn wait_for_prometheus_target(
    prometheus_url: &str,
    target_substring: &str,
    timeout_secs: u64,
) -> Result<Vec<String>> {
    let target_sub = target_substring.to_string();
    let backoff = ConstantBuilder::default()
        .with_delay(Duration::from_secs(3))
        .with_max_times((timeout_secs / 3) as usize);

    let job_names = (|| async {
        let jobs = get_prometheus_active_targets(prometheus_url).await?;
        if !jobs.iter().any(|j| j.contains(&target_sub)) {
            anyhow::bail!(
                "Target containing '{}' not found in Prometheus. Current targets: {:?}",
                target_sub,
                jobs
            );
        }
        Ok(jobs)
    })
    .retry(backoff)
    .await
    .with_context(|| {
        format!(
            "Timed out waiting for Prometheus target '{}' after {}s",
            target_substring, timeout_secs
        )
    })?;

    Ok(job_names)
}

/// Query Grafana's datasource proxy to run the same PromQL query the reth-overview
/// dashboard variable uses (`reth_info`), and extract instance labels.
///
/// This tests the full Grafana → Prometheus → metrics pipeline that dashboard
/// variable dropdowns rely on.
async fn get_grafana_reth_instances(grafana_url: &str) -> Result<Vec<String>> {
    let client = rpc::create_client()?;
    // Query through Grafana's datasource proxy (datasource ID 1 = default Prometheus)
    let url = format!(
        "{}api/datasources/proxy/1/api/v1/query?query=reth_info",
        grafana_url
    );
    let resp: serde_json::Value = client
        .get(&url)
        .header("Authorization", "Basic YWRtaW46YWRtaW4=") // admin:admin
        .send()
        .await
        .context("Failed to query Grafana datasource proxy")?
        .json()
        .await
        .context("Failed to parse Grafana datasource proxy response")?;

    let results = resp["data"]["result"]
        .as_array()
        .context("No result array in Grafana datasource proxy response")?;

    // Extract instance labels — same as the dashboard variable regex
    let instances: Vec<String> = results
        .iter()
        .filter_map(|r| r["metric"]["instance"].as_str().map(String::from))
        .collect();

    Ok(instances)
}

/// Wait until Grafana's datasource proxy returns an instance matching the expected value.
async fn wait_for_grafana_variable_instances(
    grafana_url: &str,
    expected_instance: &str,
    timeout_secs: u64,
) -> Result<Vec<String>> {
    let expected = expected_instance.to_string();
    let backoff = ConstantBuilder::default()
        .with_delay(Duration::from_secs(5))
        .with_max_times((timeout_secs / 5) as usize);

    (|| async {
        let instances = get_grafana_reth_instances(grafana_url).await?;
        if !instances.iter().any(|i| i == &expected) {
            anyhow::bail!(
                "Instance '{}' not found via Grafana datasource proxy. Current instances: {:?}",
                expected,
                instances
            );
        }
        Ok(instances)
    })
    .retry(backoff)
    .await
    .with_context(|| {
        format!(
            "Timed out waiting for Grafana to resolve instance '{}' after {}s",
            expected_instance, timeout_secs
        )
    })
}

/// Test that Prometheus picks up a dynamically added validator node.
///
/// 1. Deploy a network with monitoring (1 sequencer + 1 validator)
/// 2. Verify Prometheus has scrape targets for the initial nodes
/// 3. Add a new validator via node lifecycle
/// 4. Verify Prometheus picks up the new validator's scrape targets
/// 5. Verify Grafana can resolve the new instance via its datasource proxy
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_prometheus_picks_up_added_node() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("mon-add");
    tracing::info!(
        "=== Starting monitoring + add node test: {} ===",
        ctx.network_name
    );

    // Step 1: Deploy with monitoring enabled
    let (_docker, deployment) = deploy_with_monitoring(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let monitoring = deployment
        .monitoring
        .as_ref()
        .context("Monitoring should be enabled")?;

    let prometheus_host_url = monitoring
        .prometheus
        .host_url
        .as_ref()
        .context("Prometheus should have a host URL")?
        .to_string();

    tracing::info!(url = %prometheus_host_url, "Prometheus host URL");

    // Step 2: Verify initial targets are scraped by Prometheus
    let initial_jobs =
        wait_for_prometheus_target(&prometheus_host_url, "op-reth-validator-1", 60).await?;

    tracing::info!(?initial_jobs, "Initial Prometheus targets");

    // Verify we have the expected initial targets
    assert!(
        initial_jobs.iter().any(|j| j == "op-reth"),
        "Should have op-reth sequencer target, got: {:?}",
        initial_jobs
    );
    assert!(
        initial_jobs.iter().any(|j| j == "op-reth-validator-1"),
        "Should have op-reth-validator-1 target, got: {:?}",
        initial_jobs
    );

    // Verify NO validator-2 target exists yet
    assert!(
        !initial_jobs.iter().any(|j| j.contains("validator-2")),
        "Should NOT have validator-2 target before adding node, got: {:?}",
        initial_jobs
    );

    // Step 3: Add a new validator
    tracing::info!("Adding new validator node...");
    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;

    let mut docker = lifecycle_docker(&deployer).await?;
    let _handler = node_lifecycle::add_validator(&mut deployer, &mut docker).await?;

    // After add_validator, Prometheus was restarted and got a new host port.
    // Discover the new port by inspecting the container.
    let prom_container = &deployer.monitoring.prometheus.container_name;
    let prom_port_key = format!("{}/tcp", deployer.monitoring.prometheus.port);
    let bound_ports = docker.get_container_bound_ports(prom_container).await?;
    let new_prom_host_port = bound_ports
        .get(&prom_port_key)
        .context("Prometheus host port not found after restart")?;
    let prometheus_host_url = format!("http://localhost:{}/", new_prom_host_port);

    tracing::info!(url = %prometheus_host_url, "Prometheus host URL after restart");

    // Verify the config file has the new target
    let prom_config_path = deployer.outdata.join("monitoring/prometheus.yml");
    let after_config = tokio::fs::read_to_string(&prom_config_path).await?;
    assert!(
        after_config.contains("validator-2"),
        "prometheus.yml should contain validator-2 after add_validator"
    );

    tracing::info!("New validator added");

    // Step 4: Verify Prometheus picks up the new validator's targets
    let updated_jobs =
        wait_for_prometheus_target(&prometheus_host_url, "op-reth-validator-2", 90).await?;

    tracing::info!(
        ?updated_jobs,
        "Updated Prometheus targets after adding node"
    );

    assert!(
        updated_jobs.iter().any(|j| j == "op-reth-validator-2"),
        "Prometheus should have op-reth-validator-2 target after adding node, got: {:?}",
        updated_jobs
    );
    assert!(
        updated_jobs.iter().any(|j| j == "kona-node-validator-2"),
        "Prometheus should have kona-node-validator-2 target after adding node, got: {:?}",
        updated_jobs
    );

    // Step 5: Verify Grafana can resolve the new instance via its datasource proxy.
    // This replicates the reth-overview dashboard's `instance` variable query:
    //   query_result(reth_info) with regex /.*instance="([^"]*).*/
    // by querying Prometheus through Grafana's datasource proxy API.
    let grafana_host_url = monitoring
        .grafana
        .host_url
        .as_ref()
        .context("Grafana should have a host URL")?
        .to_string();

    tracing::info!(url = %grafana_host_url, "Grafana host URL");

    let new_validator_reth_name = &deployer.l2_stack.validators[1].op_reth.container_name;
    let expected_instance = format!("{}:9001", new_validator_reth_name);

    let grafana_instances =
        wait_for_grafana_variable_instances(&grafana_host_url, &expected_instance, 90).await?;

    tracing::info!(
        ?grafana_instances,
        "Grafana reth_info instances after adding node"
    );

    assert!(
        grafana_instances.iter().any(|i| i == &expected_instance),
        "Grafana should resolve the new validator's instance '{}' via datasource proxy, got: {:?}",
        expected_instance,
        grafana_instances
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_prometheus_picks_up_added_node passed ===");
    Ok(())
}

/// Test that Prometheus TSDB data persists across container restarts.
///
/// 1. Deploy a network with monitoring
/// 2. Wait for Prometheus to scrape enough data points
/// 3. Record the sample count for a known metric
/// 4. Add a validator (triggers Prometheus container stop + recreate)
/// 5. Verify historical samples still exist after the restart
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_prometheus_tsdb_persists_across_restart() -> Result<()> {
    let _permit = TEST_SEMAPHORE.acquire().await.context("test semaphore")?;
    init_test_tracing();

    let ctx = TestContext::new("mon-tsdb");
    tracing::info!(
        "=== Starting TSDB persistence test: {} ===",
        ctx.network_name
    );

    // Step 1: Deploy with monitoring enabled
    let (_docker, deployment) = deploy_with_monitoring(&ctx).await?;
    wait_for_all_nodes(&deployment).await;

    let monitoring = deployment
        .monitoring
        .as_ref()
        .context("Monitoring should be enabled")?;

    let prometheus_host_url = monitoring
        .prometheus
        .host_url
        .as_ref()
        .context("Prometheus should have a host URL")?
        .to_string();

    // Step 2: Wait for Prometheus to scrape some data points.
    // `up` is a built-in metric that Prometheus records for every scrape target.
    let metric = "up";
    let range = "1h";

    let pre_restart_count = (|| async {
        let count = query_prometheus_sample_count(&prometheus_host_url, metric, range).await?;
        if count < 2 {
            anyhow::bail!("Not enough samples yet (need >= 2, got {})", count);
        }
        Ok(count)
    })
    .retry(
        ConstantBuilder::default()
            .with_delay(Duration::from_secs(5))
            .with_max_times(24),
    )
    .await
    .context("Timed out waiting for Prometheus to collect enough samples")?;

    tracing::info!(
        pre_restart_count,
        "Prometheus has scraped enough samples before restart"
    );

    // Step 3: Add a validator — this triggers Prometheus container stop + recreate
    tracing::info!("Adding validator to trigger Prometheus restart...");
    let mut deployer = Deployer::load_from_file(&ctx.outdata_path)?;
    let mut docker = lifecycle_docker(&deployer).await?;
    let _handler = node_lifecycle::add_validator(&mut deployer, &mut docker).await?;

    // Discover the new Prometheus host port after restart
    let prom_container = &deployer.monitoring.prometheus.container_name;
    let prom_port_key = format!("{}/tcp", deployer.monitoring.prometheus.port);
    let bound_ports = docker.get_container_bound_ports(prom_container).await?;
    let new_prom_host_port = bound_ports
        .get(&prom_port_key)
        .context("Prometheus host port not found after restart")?;
    let prometheus_host_url = format!("http://localhost:{}/", new_prom_host_port);

    tracing::info!(url = %prometheus_host_url, "Prometheus host URL after restart");

    // Step 4: Verify historical data survived the restart.
    // Wait for Prometheus to be ready and then check that the sample count is
    // at least as large as before the restart (it may have grown due to new scrapes).
    let post_restart_count = (|| async {
        let count = query_prometheus_sample_count(&prometheus_host_url, metric, range).await?;
        if count < pre_restart_count {
            anyhow::bail!(
                "TSDB data lost! Pre-restart samples: {}, post-restart samples: {}",
                pre_restart_count,
                count
            );
        }
        Ok(count)
    })
    .retry(
        ConstantBuilder::default()
            .with_delay(Duration::from_secs(3))
            .with_max_times(20),
    )
    .await
    .context("Prometheus TSDB data did not persist across restart")?;

    tracing::info!(
        pre_restart_count,
        post_restart_count,
        "TSDB data persisted across Prometheus restart"
    );

    assert!(
        post_restart_count >= pre_restart_count,
        "Post-restart sample count ({}) should be >= pre-restart count ({})",
        post_restart_count,
        pre_restart_count
    );

    tracing::info!("=== Cleaning up ===");
    drop(docker);
    drop(_docker);
    cleanup_by_prefix(&ctx.network_name).await?;
    tracing::info!("=== test_prometheus_tsdb_persists_across_restart passed ===");
    Ok(())
}
