use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use dashmap::DashMap;
use metrics::{counter, describe_counter, describe_gauge, gauge};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use sentinel_policy::{
    build_snapshot, build_snapshot_with_heuristics, gravity_pull,
    heuristics, presets, SentinelPolicyEngine,
};
use sentinel_resolver::{
    mdns::{self, ClientNameMap},
    run_dns_listener, run_tcp_dns_listener, SentinelResolver, UpstreamConfig,
};
use sentinel_storage::{
    query_logs_merged, query_stats_merged, BufferedLogIngestor, CheckpointConfig, DuckDbLogStore,
    RamLogRing, SqliteConfigStore,
};
use sentinel_types::{
    is_valid_domain, is_valid_mac, AdlistKind, AdlistStore, BlockMode, ClientStore, ConfigStore,
    DevicePolicy, DnsProtocol, DomainRuleKind, DomainRuleStore, GroupStore, PolicyEngine,
    RiskPolicyMode,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Clone)]
struct AppState {
    config_store: Arc<SqliteConfigStore>,
    log_store: Arc<DuckDbLogStore>,
    ram_ring: Arc<RamLogRing>,
    resolver: Arc<SentinelResolver>,
    policy_engine: Arc<SentinelPolicyEngine>,
    log_ingestor: BufferedLogIngestor,
    metrics_handle: PrometheusHandle,
    admin_token: String,
    rate_limiter: Arc<SimpleRateLimiter>,
    block_mode: BlockMode,
    heuristics_enabled: Arc<std::sync::atomic::AtomicBool>,
    client_names: ClientNameMap,
}

#[derive(Default)]
struct SimpleRateLimiter {
    buckets: DashMap<String, (u32, Instant)>,
}

impl SimpleRateLimiter {
    fn check(&self, key: &str, max_per_window: u32, window: Duration) -> bool {
        let now = Instant::now();
        let mut allowed = false;
        self.buckets
            .entry(key.to_string())
            .and_modify(|(count, start)| {
                if now.duration_since(*start) > window {
                    *count = 1;
                    *start = now;
                    allowed = true;
                } else if *count < max_per_window {
                    *count += 1;
                    allowed = true;
                }
            })
            .or_insert_with(|| {
                allowed = true;
                (1, now)
            });
        allowed
    }

    fn cleanup_expired(&self, window: Duration) {
        let now = Instant::now();
        self.buckets
            .retain(|_, (_, start)| now.duration_since(*start) <= window);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    init_metrics()?;

    let require_auth = std::env::var("SENTINEL_REQUIRE_AUTH")
        .unwrap_or_default()
        .eq_ignore_ascii_case("true");

    let admin_token = std::env::var("SENTINEL_ADMIN_TOKEN").unwrap_or_default();
    if admin_token.is_empty() {
        if require_auth {
            anyhow::bail!(
                "SENTINEL_REQUIRE_AUTH is set but SENTINEL_ADMIN_TOKEN is empty. \
                 Set SENTINEL_ADMIN_TOKEN or disable SENTINEL_REQUIRE_AUTH."
            );
        }
        tracing::warn!(
            "SENTINEL_ADMIN_TOKEN is not set — admin endpoints are unprotected. \
             Set this environment variable before production use."
        );
    }

    let db_dir = std::env::var("SENTINEL_DB_DIR").unwrap_or_else(|_| ".".to_string());
    let sqlite_path = format!("{}/sentinel-config.db", db_dir);
    let duckdb_path = format!("{}/sentinel-logs.duckdb", db_dir);

    let block_mode = match std::env::var("SENTINEL_BLOCK_MODE")
        .unwrap_or_else(|_| "nxdomain".to_string())
        .as_str()
    {
        "null_ip" | "nullip" => BlockMode::NullIp,
        _ => BlockMode::Nxdomain,
    };

    let config_store = Arc::new(SqliteConfigStore::new(&sqlite_path)?);

    // Build initial policy from file blocklist + any stored domain rules
    let blocklist_path = std::env::var("SENTINEL_BLOCKLIST_PATH")
        .unwrap_or_else(|_| "fixtures/blocklist.txt".to_string());

    let mut initial_block_domains = Vec::new();
    if std::path::Path::new(&blocklist_path).exists() {
        let file = std::fs::File::open(&blocklist_path)?;
        let reader = std::io::BufReader::new(file);
        use std::io::BufRead;
        for line in reader.lines() {
            if let Some(d) = sentinel_policy::parse_blocklist_line(&line?) {
                initial_block_domains.push(d);
            }
        }
        tracing::info!(
            path = %blocklist_path,
            domains = initial_block_domains.len(),
            "loaded file blocklist"
        );
    }

    let domain_rules = {
        let store = Arc::clone(&config_store);
        tokio::task::spawn_blocking(move || DomainRuleStore::list_domain_rules(store.as_ref()))
            .await??
    };

    let heuristics_on = !std::env::var("SENTINEL_HEURISTICS")
        .unwrap_or_default()
        .eq_ignore_ascii_case("false");

    let initial_snapshot = build_snapshot_with_heuristics(
        block_mode,
        initial_block_domains,
        Vec::new(),
        &domain_rules,
        heuristics_on,
    );
    let policy_engine = Arc::new(SentinelPolicyEngine::new(initial_snapshot));
    let heuristics_enabled = Arc::new(std::sync::atomic::AtomicBool::new(heuristics_on));

    // Auto-seed preset blocklists on first boot
    {
        let store = Arc::clone(&config_store);
        let existing = tokio::task::spawn_blocking({
            let s = Arc::clone(&store);
            move || AdlistStore::list_adlists(s.as_ref())
        })
        .await??;

        if existing.is_empty() {
            tracing::info!("first boot detected — seeding {} preset blocklists", presets::PRESET_LISTS.len());
            for preset in presets::PRESET_LISTS {
                let s = Arc::clone(&store);
                let url = preset.url.to_string();
                let name = preset.name.to_string();
                let kind = match preset.kind {
                    presets::PresetKind::Block => AdlistKind::Block,
                    presets::PresetKind::Allow => AdlistKind::Allow,
                };
                let _ = tokio::task::spawn_blocking(move || {
                    AdlistStore::create_adlist(s.as_ref(), &url, &name, kind)
                })
                .await?;
            }

            // Seed built-in regex rules
            for builtin in presets::BUILTIN_REGEX_RULES {
                let s = Arc::clone(&store);
                let value = builtin.pattern.to_string();
                let comment = builtin.comment.to_string();
                let _ = tokio::task::spawn_blocking(move || {
                    DomainRuleStore::create_domain_rule(
                        s.as_ref(),
                        DomainRuleKind::RegexDeny,
                        &value,
                        Some(&comment),
                    )
                })
                .await?;
            }

            tracing::info!("preset lists seeded — triggering first gravity pull in background");
        }
    }

    let upstream_config = parse_upstream_config();
    let resolver = Arc::new(SentinelResolver::with_upstream(
        Arc::clone(&policy_engine) as Arc<dyn PolicyEngine>,
        block_mode,
        &upstream_config,
    ));
    tracing::info!(upstream = ?upstream_config, "configured upstream DNS");

    let log_store = Arc::new(DuckDbLogStore::new(&duckdb_path)?);

    let ram_capacity: usize = std::env::var("SENTINEL_RAM_LOG_CAPACITY")
        .unwrap_or_else(|_| "100000".to_string())
        .parse()
        .unwrap_or(100_000);
    let ram_ring = Arc::new(RamLogRing::new(ram_capacity));
    let log_ingestor = BufferedLogIngestor::new(Arc::clone(&ram_ring), 20_000);

    // Checkpoint config: flush RAM → DuckDB every N seconds (default 900 = 15 min)
    let checkpoint_interval_secs: u64 = std::env::var("SENTINEL_CHECKPOINT_SECS")
        .unwrap_or_else(|_| "900".to_string())
        .parse()
        .unwrap_or(900);
    let checkpoint_max_batch: usize = std::env::var("SENTINEL_CHECKPOINT_BATCH")
        .unwrap_or_else(|_| "50000".to_string())
        .parse()
        .unwrap_or(50_000);
    let checkpoint_config = CheckpointConfig {
        interval: Duration::from_secs(checkpoint_interval_secs),
        max_batch: checkpoint_max_batch,
    };

    let shutdown_notify = Arc::new(Notify::new());

    // Spawn checkpoint loop
    {
        let ck_ring = Arc::clone(&ram_ring);
        let ck_store = Arc::clone(&log_store) as Arc<dyn sentinel_types::LogStore>;
        let ck_shutdown = Arc::clone(&shutdown_notify);
        tokio::spawn(async move {
            sentinel_storage::run_checkpoint_loop(ck_ring, ck_store, checkpoint_config, ck_shutdown)
                .await;
        });
    }

    tracing::info!(
        ram_capacity,
        checkpoint_interval_secs,
        "RAM-first log pipeline active — SD card writes every {} min",
        checkpoint_interval_secs / 60
    );

    let metrics_handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Prefix("sentinel_response_time_ms".to_string()),
            &[1.0, 2.0, 5.0, 10.0, 20.0, 50.0],
        )?
        .install_recorder()?;

    let rate_limiter = Arc::new(SimpleRateLimiter::default());

    let rate_limiter_cleanup = Arc::clone(&rate_limiter);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(120));
        loop {
            interval.tick().await;
            rate_limiter_cleanup.cleanup_expired(Duration::from_secs(60));
        }
    });

    // mDNS client discovery
    let client_names: ClientNameMap = Arc::new(dashmap::DashMap::new());
    {
        let map = Arc::clone(&client_names);
        tokio::spawn(async move {
            if let Err(e) = mdns::run_mdns_listener(map).await {
                tracing::warn!(error = %e, "mDNS listener exited");
            }
        });
    }

    let state = AppState {
        config_store,
        log_store,
        ram_ring,
        resolver: Arc::clone(&resolver),
        policy_engine: Arc::clone(&policy_engine),
        log_ingestor,
        metrics_handle,
        admin_token,
        rate_limiter,
        block_mode,
        heuristics_enabled: Arc::clone(&heuristics_enabled),
        client_names,
    };

    // DNS listener
    let dns_bind: SocketAddr = std::env::var("SENTINEL_DNS_BIND")
        .unwrap_or_else(|_| "0.0.0.0:5353".to_string())
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 5353)));

    let (dns_log_tx, mut dns_log_rx) =
        tokio::sync::mpsc::channel::<sentinel_types::DnsLogRecord>(10_000);

    let dns_ingestor = state.log_ingestor.clone();
    tokio::spawn(async move {
        while let Some(record) = dns_log_rx.recv().await {
            let _ = dns_ingestor.push(record).await;
        }
    });

    let dns_resolver = Arc::clone(&resolver);
    let tcp_dns_resolver = Arc::clone(&resolver);

    let (tcp_log_tx, mut tcp_log_rx) =
        tokio::sync::mpsc::channel::<sentinel_types::DnsLogRecord>(10_000);
    let tcp_dns_ingestor = state.log_ingestor.clone();
    tokio::spawn(async move {
        while let Some(record) = tcp_log_rx.recv().await {
            let _ = tcp_dns_ingestor.push(record).await;
        }
    });

    tokio::spawn(async move {
        if let Err(e) = run_dns_listener(dns_bind, dns_resolver, Some(dns_log_tx)).await {
            tracing::error!(error = %e, "UDP DNS listener exited with error");
        }
    });

    tokio::spawn(async move {
        if let Err(e) =
            run_tcp_dns_listener(dns_bind, tcp_dns_resolver, Some(tcp_log_tx)).await
        {
            tracing::error!(error = %e, "TCP DNS listener exited with error");
        }
    });

    // Gravity scheduler
    let gravity_interval_secs: u64 = std::env::var("SENTINEL_GRAVITY_INTERVAL_SECS")
        .unwrap_or_else(|_| "604800".to_string()) // 1 week
        .parse()
        .unwrap_or(604800);

    // First-boot gravity: pull immediately if we just seeded presets
    {
        let first_boot_state = state.clone();
        let store_check = Arc::clone(&first_boot_state.config_store);
        let has_lists = tokio::task::spawn_blocking(move || {
            AdlistStore::list_adlists(store_check.as_ref())
                .map(|l| l.iter().any(|a| a.domain_count > 0))
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false);

        if !has_lists {
            let gstate = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                tracing::info!("first-boot gravity: pulling blocklists now");
                if let Err(e) = run_gravity_update(&gstate).await {
                    tracing::error!(error = %e, "first-boot gravity pull failed");
                }
            });
        }
    }

    if gravity_interval_secs > 0 {
        let gstate = state.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(gravity_interval_secs));
            interval.tick().await; // skip immediate first tick
            loop {
                interval.tick().await;
                tracing::info!("gravity scheduler: starting automatic update");
                if let Err(e) = run_gravity_update(&gstate).await {
                    tracing::error!(error = %e, "gravity automatic update failed");
                }
            }
        });
    }

    let app = Router::new()
        .route("/healthz", get(health))
        .route("/metrics", get(metrics_handler))
        // Devices
        .route("/api/devices", get(list_devices).post(upsert_device))
        .route("/api/devices/{id}", get(get_device))
        // Resolve + logs
        .route("/api/resolve", post(resolve_domain))
        .route("/api/logs", get(query_logs))
        .route("/api/logs/live", get(live_tail_sse))
        .route("/api/stats", get(query_stats))
        // Adlists + gravity
        .route("/api/lists", get(list_adlists).post(create_adlist))
        .route("/api/lists/{id}", delete(delete_adlist))
        .route("/api/lists/{id}/toggle", put(toggle_adlist))
        .route("/api/gravity/update", post(trigger_gravity))
        .route("/api/gravity/status", get(gravity_status))
        // Domain rules
        .route(
            "/api/domains",
            get(list_domain_rules).post(create_domain_rule),
        )
        .route("/api/domains/{id}", delete(delete_domain_rule))
        .route("/api/domains/{id}/toggle", put(toggle_domain_rule))
        // Groups
        .route("/api/groups", get(list_groups).post(create_group))
        .route("/api/groups/{id}", delete(delete_group))
        .route(
            "/api/groups/{group_id}/adlists/{adlist_id}",
            put(assign_adlist_to_group).delete(remove_adlist_from_group),
        )
        .route(
            "/api/groups/{group_id}/rules/{rule_id}",
            put(assign_rule_to_group).delete(remove_rule_from_group),
        )
        // Clients
        .route("/api/clients", get(list_clients).post(upsert_client))
        .route("/api/clients/{id}", delete(delete_client))
        // Client discovery
        .route("/api/discovered-clients", get(list_discovered_clients))
        // Heuristic scoring
        .route("/api/heuristics/score", post(score_domain_heuristic))
        .route("/api/heuristics/toggle", put(toggle_heuristics))
        .route("/api/heuristics/status", get(heuristics_status))
        // Config export/import
        .route("/api/config/export", get(export_config))
        .route("/api/config/import", post(import_config))
        // Wireguard (stub)
        .route("/api/wireguard/{id}", post(generate_wireguard_profile))
        .with_state(state);

    let api_bind: SocketAddr = std::env::var("SENTINEL_API_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 8080)));

    tracing::info!("sentinel-control-plane API on {}", api_bind);

    let listener = tokio::net::TcpListener::bind(api_bind).await?;
    let shutdown_notify_signal = Arc::clone(&shutdown_notify);
    let shutdown_signal = async move {
        #[cfg(unix)]
        {
            let mut sigterm = tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::terminate(),
            )
            .expect("failed to register SIGTERM handler");
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm.recv() => {}
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl+c");
        }
        tracing::info!("shutdown signal received — triggering emergency flush");
        shutdown_notify_signal.notify_one();
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await?;

    // Give the checkpoint loop time to complete the emergency flush
    tokio::time::sleep(Duration::from_secs(3)).await;
    tracing::info!("shutdown complete");
    Ok(())
}

fn init_metrics() -> anyhow::Result<()> {
    describe_counter!("sentinel_queries_total", "Total DNS queries processed");
    describe_counter!("sentinel_blocked_total", "Total blocked DNS queries");
    describe_gauge!("sentinel_queue_depth", "Buffered queue depth estimate");
    describe_gauge!("sentinel_cache_size", "DNS cache entry count");
    Ok(())
}

// ─── Health / Metrics ───

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    gauge!("sentinel_cache_size").set(state.resolver.cache_len() as f64);
    state.metrics_handle.render()
}

// ─── Device policy endpoints ───

#[derive(Debug, Deserialize)]
struct DevicePolicyInput {
    id: Option<Uuid>,
    mac_address: String,
    group_memberships: Vec<String>,
    wireguard_enabled: bool,
    risk_policy_mode: String,
}

async fn upsert_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DevicePolicyInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    if !is_valid_mac(&input.mac_address) {
        return Err((
            StatusCode::BAD_REQUEST,
            "invalid mac_address format, expected AA:BB:CC:DD:EE:FF".to_string(),
        ));
    }
    let mode = match input.risk_policy_mode.as_str() {
        "block" => RiskPolicyMode::Block,
        "bypass" => RiskPolicyMode::Bypass,
        _ => return Err((StatusCode::BAD_REQUEST, "invalid risk_policy_mode".to_string())),
    };
    let policy = DevicePolicy {
        id: input.id.unwrap_or_else(Uuid::new_v4),
        mac_address: input.mac_address,
        group_memberships: input.group_memberships,
        wireguard_enabled: input.wireguard_enabled,
        risk_policy_mode: mode,
    };
    tokio::task::spawn_blocking({
        let store = Arc::clone(&state.config_store);
        let p = policy.clone();
        move || ConfigStore::upsert_device_policy(store.as_ref(), p)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(policy)))
}

async fn list_devices(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let rows = tokio::task::spawn_blocking({
        let store = Arc::clone(&state.config_store);
        move || ConfigStore::list_device_policies(store.as_ref())
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(Json(rows))
}

async fn get_device(
    Path(id): Path<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let row = tokio::task::spawn_blocking({
        let store = Arc::clone(&state.config_store);
        move || ConfigStore::get_device_policy(store.as_ref(), id)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    match row {
        Some(policy) => Ok((StatusCode::OK, Json(policy)).into_response()),
        None => Ok((StatusCode::NOT_FOUND, "device not found").into_response()),
    }
}

// ─── Resolve + logs ───

#[derive(Debug, Deserialize)]
struct ResolveRequest {
    client_id: String,
    query_domain: String,
    protocol: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResolveResponse {
    action: String,
    cname_chain: Vec<String>,
    response_time_ms: u64,
}

async fn resolve_domain(
    State(state): State<AppState>,
    Json(req): Json<ResolveRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !state
        .rate_limiter
        .check("global_resolve", 600, Duration::from_secs(60))
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "global rate limit exceeded".to_string(),
        ));
    }
    if !is_valid_domain(&req.query_domain) {
        return Err((StatusCode::BAD_REQUEST, "invalid domain name".to_string()));
    }
    let protocol = match req.protocol.as_deref() {
        Some("doh") => DnsProtocol::Doh,
        Some("dot") => DnsProtocol::Dot,
        _ => DnsProtocol::Udp,
    };
    counter!("sentinel_queries_total").increment(1);
    let result = state
        .resolver
        .resolve_domain(&req.query_domain)
        .await
        .map_err(internal_error)?;
    if matches!(result.action, sentinel_types::DnsAction::Blocked) {
        counter!("sentinel_blocked_total").increment(1);
    }
    gauge!("sentinel_queue_depth").set(state.log_ingestor.queue_depth() as f64);
    let response = ResolveResponse {
        action: result.action.as_str().to_string(),
        cname_chain: result.cname_chain.clone(),
        response_time_ms: result.response_time_ms,
    };
    let row =
        SentinelResolver::into_log_record(&req.client_id, &req.query_domain, protocol, &result);
    state.log_ingestor.push(row).await.map_err(internal_error)?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
struct LogQueryParams {
    limit: Option<u32>,
    offset: Option<u32>,
    domain: Option<String>,
    action: Option<String>,
}

async fn query_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<LogQueryParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0);
    let domain_filter = params.domain.clone();
    let action_filter = params.action.clone();
    let store = Arc::clone(&state.log_store);
    let ring = Arc::clone(&state.ram_ring);
    let result = tokio::task::spawn_blocking(move || {
        query_logs_merged(
            &ring,
            &store,
            limit,
            offset,
            domain_filter.as_deref(),
            action_filter.as_deref(),
        )
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(Json(result))
}

async fn query_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.log_store);
    let ring = Arc::clone(&state.ram_ring);
    let stats = tokio::task::spawn_blocking(move || query_stats_merged(&ring, &store))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(stats))
}

// ─── SSE Live Tail ───

async fn live_tail_sse(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, String)>
{
    authorize_admin(&headers, &state)?;
    let mut rx = state.log_ingestor.subscribe_live();
    let client_names = Arc::clone(&state.client_names);

    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(record) => {
                    // Enrich client_id with mDNS-discovered hostname
                    let display_name = record.client_id.parse::<std::net::IpAddr>()
                        .ok()
                        .and_then(|ip| client_names.get(&ip).map(|c| c.hostname.clone()));

                    let entry = serde_json::json!({
                        "timestamp": record.timestamp.to_rfc3339(),
                        "client_id": record.client_id,
                        "client_name": display_name,
                        "query_domain": record.query_domain,
                        "action": record.action.as_str(),
                        "protocol": record.protocol.as_str(),
                        "response_time_ms": record.response_time_ms,
                    });
                    if let Ok(data) = serde_json::to_string(&entry) {
                        yield Ok(Event::default().data(data));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let msg = format!("{{\"warning\":\"skipped {} events (client too slow)\"}}", n);
                    yield Ok(Event::default().event("lag").data(msg));
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    ))
}

// ─── Adlist endpoints ───

#[derive(Debug, Deserialize)]
struct CreateAdlistInput {
    url: String,
    name: Option<String>,
    kind: Option<String>,
}

async fn list_adlists(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let lists = tokio::task::spawn_blocking(move || AdlistStore::list_adlists(store.as_ref()))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(lists))
}

async fn create_adlist(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateAdlistInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let kind = match input.kind.as_deref() {
        Some("allow") => AdlistKind::Allow,
        _ => AdlistKind::Block,
    };
    let name = input.name.unwrap_or_default();
    let store = Arc::clone(&state.config_store);
    let list = tokio::task::spawn_blocking(move || {
        AdlistStore::create_adlist(store.as_ref(), &input.url, &name, kind)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(list)))
}

async fn delete_adlist(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || AdlistStore::delete_adlist(store.as_ref(), id))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct ToggleInput {
    enabled: bool,
}

async fn toggle_adlist(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ToggleInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        AdlistStore::toggle_adlist(store.as_ref(), id, input.enabled)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

// ─── Gravity ───

#[derive(Serialize)]
struct GravityResponse {
    message: String,
    lists_processed: usize,
    total_block_domains: usize,
    total_allow_domains: usize,
}

#[derive(Serialize)]
struct GravityStatusResponse {
    bootstrap_index_path: String,
    bootstrap_index_present: bool,
    bootstrap_index_age_secs: Option<u64>,
    last_gravity_sync: Option<String>,
}

async fn trigger_gravity(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let result = run_gravity_update(&state).await.map_err(internal_error)?;
    Ok(Json(result))
}

async fn gravity_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;

    let bootstrap_path =
        std::env::var("SENTINEL_BOOTSTRAP_INDEX_PATH").unwrap_or_else(|_| "default.fst".to_string());
    let bootstrap_meta = std::fs::metadata(&bootstrap_path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|d| d.as_secs());

    let store = Arc::clone(&state.config_store);
    let last_gravity_sync = tokio::task::spawn_blocking(move || {
        AdlistStore::list_adlists(store.as_ref()).ok().and_then(|lists| {
            lists
                .into_iter()
                .filter_map(|l| l.last_updated)
                .max()
                .map(|dt| dt.to_rfc3339())
        })
    })
    .await
    .map_err(internal_error)?;

    Ok(Json(GravityStatusResponse {
        bootstrap_index_path: bootstrap_path,
        bootstrap_index_present: bootstrap_meta.is_some(),
        bootstrap_index_age_secs: bootstrap_meta,
        last_gravity_sync,
    }))
}

async fn run_gravity_update(state: &AppState) -> anyhow::Result<GravityResponse> {
    let store = Arc::clone(&state.config_store);
    let adlists = tokio::task::spawn_blocking({
        let s = Arc::clone(&store);
        move || AdlistStore::list_adlists(s.as_ref())
    })
    .await??;

    let gravity_result = gravity_pull(&adlists).await;

    // Update per-list status in DB
    for (list_id, count, status) in &gravity_result.per_list_counts {
        let s = Arc::clone(&store);
        let lid = *list_id;
        let cnt = *count;
        let st = status.clone();
        tokio::task::spawn_blocking(move || {
            AdlistStore::update_adlist_status(s.as_ref(), lid, cnt, &st)
        })
        .await??;
    }

    let domain_rules = {
        let s = Arc::clone(&store);
        tokio::task::spawn_blocking(move || DomainRuleStore::list_domain_rules(s.as_ref()))
            .await??
    };

    let new_snapshot = build_snapshot(
        state.block_mode,
        gravity_result.block_domains.clone(),
        gravity_result.allow_domains.clone(),
        &domain_rules,
    );

    // Atomic hot-swap — zero DNS downtime
    state.policy_engine.swap_snapshot(new_snapshot);

    tracing::info!(
        block_domains = gravity_result.block_domains.len(),
        allow_domains = gravity_result.allow_domains.len(),
        lists = gravity_result.per_list_counts.len(),
        "gravity update complete"
    );

    Ok(GravityResponse {
        message: "gravity update complete".to_string(),
        lists_processed: gravity_result.per_list_counts.len(),
        total_block_domains: gravity_result.block_domains.len(),
        total_allow_domains: gravity_result.allow_domains.len(),
    })
}

// ─── Domain rule endpoints ───

#[derive(Debug, Deserialize)]
struct CreateDomainRuleInput {
    kind: String,
    value: String,
    comment: Option<String>,
}

async fn list_domain_rules(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let rules =
        tokio::task::spawn_blocking(move || DomainRuleStore::list_domain_rules(store.as_ref()))
            .await
            .map_err(internal_error)?
            .map_err(internal_error)?;
    Ok(Json(rules))
}

async fn create_domain_rule(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateDomainRuleInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let kind = match input.kind.as_str() {
        "exact_deny" => DomainRuleKind::ExactDeny,
        "exact_allow" => DomainRuleKind::ExactAllow,
        "regex_deny" => DomainRuleKind::RegexDeny,
        "regex_allow" => DomainRuleKind::RegexAllow,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "invalid kind: must be exact_deny, exact_allow, regex_deny, or regex_allow"
                    .to_string(),
            ))
        }
    };

    if matches!(kind, DomainRuleKind::RegexDeny | DomainRuleKind::RegexAllow)
        && regex::Regex::new(&input.value).is_err()
    {
        return Err((StatusCode::BAD_REQUEST, "invalid regex pattern".to_string()));
    }

    let store = Arc::clone(&state.config_store);
    let comment = input.comment.clone();
    let val = input.value.clone();
    let rule = tokio::task::spawn_blocking(move || {
        DomainRuleStore::create_domain_rule(store.as_ref(), kind, &val, comment.as_deref())
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;

    rebuild_policy_from_rules(&state).await.map_err(internal_error)?;

    Ok((StatusCode::CREATED, Json(rule)))
}

async fn delete_domain_rule(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || DomainRuleStore::delete_domain_rule(store.as_ref(), id))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    rebuild_policy_from_rules(&state).await.map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn toggle_domain_rule(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ToggleInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        DomainRuleStore::toggle_domain_rule(store.as_ref(), id, input.enabled)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    rebuild_policy_from_rules(&state).await.map_err(internal_error)?;
    Ok(StatusCode::OK)
}

async fn rebuild_policy_from_rules(state: &AppState) -> anyhow::Result<()> {
    let store = Arc::clone(&state.config_store);
    let rules =
        tokio::task::spawn_blocking(move || DomainRuleStore::list_domain_rules(store.as_ref()))
            .await??;

    let current = state.policy_engine.load_snapshot();
    let new_snapshot = build_snapshot(
        state.block_mode,
        current
            .block_index
            .is_blocked("__collect__")
            .then(Vec::new)
            .unwrap_or_default(),
        Vec::new(),
        &rules,
    );

    // Preserve gravity data but update rules
    let mut snap = (*current).clone();
    snap.exact_allow = new_snapshot.exact_allow;
    snap.exact_deny = new_snapshot.exact_deny;
    snap.regex_index = new_snapshot.regex_index;
    state.policy_engine.swap_snapshot(snap);
    Ok(())
}

// ─── Group endpoints ───

#[derive(Debug, Deserialize)]
struct CreateGroupInput {
    name: String,
    description: Option<String>,
}

async fn list_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let groups = tokio::task::spawn_blocking(move || GroupStore::list_groups(store.as_ref()))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(groups))
}

async fn create_group(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateGroupInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let group = tokio::task::spawn_blocking(move || {
        GroupStore::create_group(store.as_ref(), &input.name, input.description.as_deref())
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(group)))
}

async fn delete_group(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || GroupStore::delete_group(store.as_ref(), id))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_adlist_to_group(
    Path((group_id, adlist_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        GroupStore::assign_adlist_to_group(store.as_ref(), adlist_id, group_id)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

async fn remove_adlist_from_group(
    Path((group_id, adlist_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        GroupStore::remove_adlist_from_group(store.as_ref(), adlist_id, group_id)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn assign_rule_to_group(
    Path((group_id, rule_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        GroupStore::assign_rule_to_group(store.as_ref(), rule_id, group_id)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(StatusCode::OK)
}

async fn remove_rule_from_group(
    Path((group_id, rule_id)): Path<(i64, i64)>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        GroupStore::remove_rule_from_group(store.as_ref(), rule_id, group_id)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Client endpoints ───

#[derive(Debug, Deserialize)]
struct UpsertClientInput {
    ip: String,
    name: Option<String>,
    group_ids: Option<Vec<i64>>,
}

async fn list_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let clients = tokio::task::spawn_blocking(move || ClientStore::list_clients(store.as_ref()))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(Json(clients))
}

async fn upsert_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<UpsertClientInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    let gids = input.group_ids.unwrap_or_default();
    let client = tokio::task::spawn_blocking(move || {
        ClientStore::upsert_client(store.as_ref(), &input.ip, input.name.as_deref(), &gids)
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    Ok((StatusCode::CREATED, Json(client)))
}

async fn delete_client(
    Path(id): Path<i64>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || ClientStore::delete_client(store.as_ref(), id))
        .await
        .map_err(internal_error)?
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Config export/import ───

#[derive(Debug, Serialize, Deserialize)]
struct SentinelConfig {
    #[serde(default)]
    adlists: Vec<AdlistConfig>,
    #[serde(default)]
    domain_rules: Vec<DomainRuleConfig>,
    #[serde(default)]
    groups: Vec<GroupConfig>,
    #[serde(default)]
    clients: Vec<ClientConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AdlistConfig {
    url: String,
    name: String,
    kind: String,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct DomainRuleConfig {
    kind: String,
    value: String,
    enabled: bool,
    comment: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GroupConfig {
    name: String,
    description: Option<String>,
    enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClientConfig {
    ip: String,
    name: Option<String>,
    group_ids: Vec<i64>,
}

async fn export_config(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let store = Arc::clone(&state.config_store);

    let (adlists, rules, groups, clients) = tokio::task::spawn_blocking(move || {
        let a = AdlistStore::list_adlists(store.as_ref())?;
        let r = DomainRuleStore::list_domain_rules(store.as_ref())?;
        let g = GroupStore::list_groups(store.as_ref())?;
        let c = ClientStore::list_clients(store.as_ref())?;
        anyhow::Ok((a, r, g, c))
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;

    let config = SentinelConfig {
        adlists: adlists
            .into_iter()
            .map(|a| AdlistConfig {
                url: a.url,
                name: a.name,
                kind: match a.kind {
                    AdlistKind::Block => "block".to_string(),
                    AdlistKind::Allow => "allow".to_string(),
                },
                enabled: a.enabled,
            })
            .collect(),
        domain_rules: rules
            .into_iter()
            .map(|r| DomainRuleConfig {
                kind: format!("{:?}", r.kind).to_ascii_lowercase(),
                value: r.value,
                enabled: r.enabled,
                comment: r.comment,
            })
            .collect(),
        groups: groups
            .into_iter()
            .map(|g| GroupConfig {
                name: g.name,
                description: g.description,
                enabled: g.enabled,
            })
            .collect(),
        clients: clients
            .into_iter()
            .map(|c| ClientConfig {
                ip: c.ip,
                name: c.name,
                group_ids: c.group_ids,
            })
            .collect(),
    };

    Ok(Json(config))
}

async fn import_config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(config): Json<SentinelConfig>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;

    let store = Arc::clone(&state.config_store);
    tokio::task::spawn_blocking(move || {
        for a in &config.adlists {
            let kind = match a.kind.as_str() {
                "allow" => AdlistKind::Allow,
                _ => AdlistKind::Block,
            };
            let _ = AdlistStore::create_adlist(store.as_ref(), &a.url, &a.name, kind);
        }
        for r in &config.domain_rules {
            let kind = match r.kind.as_str() {
                "exact_deny" | "exactdeny" => DomainRuleKind::ExactDeny,
                "exact_allow" | "exactallow" => DomainRuleKind::ExactAllow,
                "regex_deny" | "regexdeny" => DomainRuleKind::RegexDeny,
                "regex_allow" | "regexallow" => DomainRuleKind::RegexAllow,
                _ => DomainRuleKind::ExactDeny,
            };
            let _ =
                DomainRuleStore::create_domain_rule(store.as_ref(), kind, &r.value, r.comment.as_deref());
        }
        for g in &config.groups {
            let _ = GroupStore::create_group(store.as_ref(), &g.name, g.description.as_deref());
        }
        for c in &config.clients {
            let _ = ClientStore::upsert_client(store.as_ref(), &c.ip, c.name.as_deref(), &c.group_ids);
        }
        anyhow::Ok(())
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;

    Ok((StatusCode::OK, "config imported"))
}

// ─── Heuristic scoring ───

#[derive(Debug, Deserialize)]
struct ScoreDomainInput {
    domain: String,
}

#[derive(Debug, Serialize)]
struct HeuristicScoreResponse {
    domain: String,
    score: f64,
    verdict: String,
    signals: Vec<HeuristicSignalResponse>,
    threshold_block: f64,
    threshold_warn: f64,
}

#[derive(Debug, Serialize)]
struct HeuristicSignalResponse {
    name: String,
    weight: f64,
    detail: String,
}

// ─── Client discovery ───

async fn list_discovered_clients(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let entries: Vec<serde_json::Value> = state
        .client_names
        .iter()
        .map(|entry| {
            serde_json::json!({
                "ip": entry.key().to_string(),
                "hostname": entry.value().hostname,
                "device_type": entry.value().device_type.as_str(),
            })
        })
        .collect();
    Ok(Json(entries))
}

// ─── Heuristic endpoints ───

async fn score_domain_heuristic(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ScoreDomainInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let result = heuristics::score_domain(&input.domain);
    Ok(Json(HeuristicScoreResponse {
        domain: input.domain,
        score: result.score,
        verdict: result.verdict.as_str().to_string(),
        signals: result
            .signals
            .into_iter()
            .map(|s| HeuristicSignalResponse {
                name: s.name.to_string(),
                weight: s.weight,
                detail: s.detail,
            })
            .collect(),
        threshold_block: heuristics::BLOCK_THRESHOLD,
        threshold_warn: heuristics::WARN_THRESHOLD,
    }))
}

async fn toggle_heuristics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ToggleInput>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    state
        .heuristics_enabled
        .store(input.enabled, std::sync::atomic::Ordering::Relaxed);

    // Update the live policy snapshot
    let mut snap = (*state.policy_engine.load_snapshot()).clone();
    snap.heuristics_enabled = input.enabled;
    state.policy_engine.swap_snapshot(snap);

    Ok(Json(serde_json::json!({ "heuristics_enabled": input.enabled })))
}

async fn heuristics_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    let enabled = state
        .heuristics_enabled
        .load(std::sync::atomic::Ordering::Relaxed);
    Ok(Json(serde_json::json!({
        "heuristics_enabled": enabled,
        "block_threshold": heuristics::BLOCK_THRESHOLD,
        "warn_threshold": heuristics::WARN_THRESHOLD,
    })))
}

// ─── WireGuard stub ───

async fn generate_wireguard_profile(
    Path(_device_id): Path<Uuid>,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    authorize_admin(&headers, &state)?;
    Err::<(StatusCode, String), _>((
        StatusCode::NOT_IMPLEMENTED,
        "WireGuard profile generation is not yet implemented".to_string(),
    ))
}

// ─── Auth + helpers ───

fn internal_error<E: std::fmt::Display>(err: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn authorize_admin(headers: &HeaderMap, state: &AppState) -> Result<(), (StatusCode, String)> {
    if state.admin_token.is_empty() {
        return Ok(());
    }
    let provided = headers
        .get("x-admin-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if constant_time_eq(provided.as_bytes(), state.admin_token.as_bytes()) {
        return Ok(());
    }
    Err((
        StatusCode::UNAUTHORIZED,
        "invalid admin token".to_string(),
    ))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Parse SENTINEL_UPSTREAM env var.
/// Formats:
///   - (empty or unset) => system default (plain DNS)
///   - tls://1.1.1.1:853#cloudflare-dns.com
///   - https://1.1.1.1:443#cloudflare-dns.com
///   - plain://8.8.8.8:53 (explicit plain)
fn parse_upstream_config() -> UpstreamConfig {
    let raw = std::env::var("SENTINEL_UPSTREAM").unwrap_or_default();
    if raw.is_empty() || raw.starts_with("plain://") {
        return UpstreamConfig::Default;
    }

    let parse_addr_name = |rest: &str| -> Option<(SocketAddr, String)> {
        let parts: Vec<&str> = rest.splitn(2, '#').collect();
        if parts.len() != 2 {
            return None;
        }
        let addr: SocketAddr = parts[0].parse().ok()?;
        Some((addr, parts[1].to_string()))
    };

    if let Some(rest) = raw.strip_prefix("tls://") {
        if let Some((addr, tls_name)) = parse_addr_name(rest) {
            return UpstreamConfig::Tls { addr, tls_name };
        }
        tracing::warn!(raw = %raw, "invalid SENTINEL_UPSTREAM tls format, falling back to default");
    } else if let Some(rest) = raw.strip_prefix("https://") {
        if let Some((addr, tls_name)) = parse_addr_name(rest) {
            return UpstreamConfig::Https { addr, tls_name };
        }
        tracing::warn!(raw = %raw, "invalid SENTINEL_UPSTREAM https format, falling back to default");
    } else {
        tracing::warn!(raw = %raw, "unknown SENTINEL_UPSTREAM scheme, falling back to default");
    }

    UpstreamConfig::Default
}
