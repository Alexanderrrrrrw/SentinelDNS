use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use duckdb::{params as duck_params, Connection as DuckConnection};
use rusqlite::{params as sqlite_params, Connection as SqliteConnection};
use sentinel_types::{
    Adlist, AdlistKind, AdlistStore, Client, ClientStore, ConfigStore, DevicePolicy, DnsLogRecord,
    DomainRule, DomainRuleKind, DomainRuleStore, Group, GroupStore, LogStore, RiskPolicyMode,
};
use tokio::sync::{broadcast, mpsc, Notify};
use tracing::{error, info};
use uuid::Uuid;

// ─── DuckDB log store ───

pub struct DuckDbLogStore {
    conn: Mutex<DuckConnection>,
}

impl DuckDbLogStore {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = DuckConnection::open(path).context("failed to open duckdb")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS dns_logs (
              timestamp TIMESTAMP,
              client_id VARCHAR,
              query_domain VARCHAR,
              cname_chain_json VARCHAR,
              action VARCHAR,
              protocol VARCHAR,
              response_time_ms BIGINT
            );
            "#,
        )
        .context("failed to initialize duckdb schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl LogStore for DuckDbLogStore {
    fn append_dns_logs(&self, rows: &[DnsLogRecord]) -> anyhow::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("duckdb mutex poisoned"))?;
        let tx = guard
            .transaction()
            .context("duckdb transaction begin failed")?;
        for row in rows {
            tx.execute(
                "INSERT INTO dns_logs VALUES (?, ?, ?, ?, ?, ?, ?)",
                duck_params![
                    row.timestamp.to_rfc3339(),
                    row.client_id,
                    row.query_domain,
                    serde_json::to_string(&row.cname_chain)?,
                    row.action.as_str(),
                    row.protocol.as_str(),
                    row.response_time_ms as i64,
                ],
            )
            .context("duckdb insert failed")?;
        }
        tx.commit().context("duckdb transaction commit failed")?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
// RAM-FIRST LOG PIPELINE
//
// Architecture (designed to maximize SD card lifespan):
//
//   DNS query ──► mpsc channel ──► RamLogRing (VecDeque in RAM)
//                                      │
//                                      ├── Dashboard queries read from here (instant)
//                                      │
//                                      └── Checkpoint timer (every 15 min)
//                                            └── Drain to DuckDB on disk (niced)
//
//   SIGTERM ──► emergency flush ──► drain all RAM to DuckDB
//
// The RamLogRing is a bounded circular buffer. When it fills up,
// the oldest records are evicted (they've already been checkpointed).
// The dashboard queries BOTH the RAM ring AND DuckDB, merging results
// so the user always sees real-time data.
// ═══════════════════════════════════════════════════════════════════

pub struct RamLogRing {
    records: RwLock<VecDeque<DnsLogRecord>>,
    capacity: usize,
    total_ingested: AtomicU64,
    total_checkpointed: AtomicU64,
}

impl RamLogRing {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: RwLock::new(VecDeque::with_capacity(capacity.min(200_000))),
            capacity,
            total_ingested: AtomicU64::new(0),
            total_checkpointed: AtomicU64::new(0),
        }
    }

    pub fn push(&self, record: DnsLogRecord) {
        let mut ring = self.records.write().unwrap();
        if ring.len() >= self.capacity {
            ring.pop_front();
        }
        ring.push_back(record);
        self.total_ingested.fetch_add(1, Ordering::Relaxed);
    }

    /// Drain up to `max` oldest records for checkpointing to disk.
    pub fn drain_for_checkpoint(&self, max: usize) -> Vec<DnsLogRecord> {
        let mut ring = self.records.write().unwrap();
        let n = ring.len().min(max);
        let drained: Vec<DnsLogRecord> = ring.drain(..n).collect();
        self.total_checkpointed
            .fetch_add(drained.len() as u64, Ordering::Relaxed);
        drained
    }

    /// Drain ALL records (emergency flush on shutdown).
    pub fn drain_all(&self) -> Vec<DnsLogRecord> {
        let mut ring = self.records.write().unwrap();
        let drained: Vec<DnsLogRecord> = ring.drain(..).collect();
        self.total_checkpointed
            .fetch_add(drained.len() as u64, Ordering::Relaxed);
        drained
    }

    /// Read-only snapshot for queries. No disk I/O.
    pub fn snapshot(&self) -> Vec<DnsLogRecord> {
        let ring = self.records.read().unwrap();
        ring.iter().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.records.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn total_ingested(&self) -> u64 {
        self.total_ingested.load(Ordering::Relaxed)
    }

    pub fn total_checkpointed(&self) -> u64 {
        self.total_checkpointed.load(Ordering::Relaxed)
    }
}

// ─── Checkpoint engine ───

pub struct CheckpointConfig {
    pub interval: Duration,
    pub max_batch: usize,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(900), // 15 minutes
            max_batch: 50_000,
        }
    }
}

/// Runs the checkpoint loop. Awaits `shutdown_notify` to perform emergency flush.
pub async fn run_checkpoint_loop(
    ring: Arc<RamLogRing>,
    store: Arc<dyn LogStore>,
    config: CheckpointConfig,
    shutdown_notify: Arc<Notify>,
) {
    let mut interval = tokio::time::interval(config.interval);
    interval.tick().await; // skip immediate first tick

    loop {
        tokio::select! {
            _ = interval.tick() => {
                do_checkpoint(&ring, &store, config.max_batch, "scheduled");
            }
            _ = shutdown_notify.notified() => {
                info!("checkpoint: shutdown signal received, performing emergency flush");
                do_emergency_flush(&ring, &store);
                return;
            }
        }
    }
}

fn do_checkpoint(ring: &RamLogRing, store: &Arc<dyn LogStore>, max_batch: usize, reason: &str) {
    let batch = ring.drain_for_checkpoint(max_batch);
    if batch.is_empty() {
        return;
    }
    let count = batch.len();

    // Run the flush on a blocking thread so we don't stall the async runtime
    let store_clone = Arc::clone(store);
    let handle = std::thread::Builder::new()
        .name("sentinel-checkpoint".into())
        .spawn(move || {
            // Lower CPU + I/O scheduling priority on the flush thread (Linux/RPi only)
            #[cfg(target_os = "linux")]
            unsafe {
                libc::nice(10);
                // IOPRIO_CLASS_IDLE = 3, IOPRIO_WHO_THREAD = 1
                libc::syscall(libc::SYS_ioprio_set, 1i32, 0i32, (3i32 << 13) | 7i32);
            }

            if let Err(e) = store_clone.append_dns_logs(&batch) {
                error!(error = %e, count, "checkpoint flush to DuckDB failed");
            }
        });

    match handle {
        Ok(h) => {
            if let Err(e) = h.join() {
                error!("checkpoint thread panicked: {:?}", e);
            } else {
                info!(count, reason, "checkpoint: flushed to DuckDB");
            }
        }
        Err(e) => {
            error!(error = %e, "failed to spawn checkpoint thread");
        }
    }
}

fn do_emergency_flush(ring: &RamLogRing, store: &Arc<dyn LogStore>) {
    let batch = ring.drain_all();
    if batch.is_empty() {
        info!("emergency flush: RAM buffer was empty, nothing to write");
        return;
    }
    let count = batch.len();
    info!(count, "emergency flush: writing all RAM logs to DuckDB");
    if let Err(e) = store.append_dns_logs(&batch) {
        error!(error = %e, count, "EMERGENCY FLUSH FAILED — data lost");
    } else {
        info!(count, "emergency flush: complete, no data lost");
    }
}

// ─── BufferedLogIngestor (async mpsc -> RamLogRing + broadcast for SSE) ───

#[derive(Clone)]
pub struct BufferedLogIngestor {
    sender: mpsc::Sender<DnsLogRecord>,
    ring: Arc<RamLogRing>,
    live_tx: broadcast::Sender<DnsLogRecord>,
}

impl BufferedLogIngestor {
    pub fn new(ring: Arc<RamLogRing>, channel_capacity: usize) -> Self {
        let (sender, mut receiver) = mpsc::channel::<DnsLogRecord>(channel_capacity);
        // SSE live tail: 256-slot broadcast ring. If a slow client falls behind, it misses events.
        let (live_tx, _) = broadcast::channel::<DnsLogRecord>(256);
        let ring_bg = Arc::clone(&ring);
        let live_tx_bg = live_tx.clone();
        tokio::spawn(async move {
            while let Some(record) = receiver.recv().await {
                ring_bg.push(record.clone());
                let _ = live_tx_bg.send(record);
            }
            info!("log ingestor channel closed");
        });
        Self { sender, ring, live_tx }
    }

    pub async fn push(&self, row: DnsLogRecord) -> anyhow::Result<()> {
        self.sender
            .send(row)
            .await
            .map_err(|_| anyhow::anyhow!("buffered log ingestor channel closed"))
    }

    pub fn queue_depth(&self) -> usize {
        self.ring.len()
    }

    pub fn ring(&self) -> &Arc<RamLogRing> {
        &self.ring
    }

    /// Subscribe to real-time log events for SSE streaming.
    pub fn subscribe_live(&self) -> broadcast::Receiver<DnsLogRecord> {
        self.live_tx.subscribe()
    }
}

// ─── Merged query engine (RAM + DuckDB) ───

impl DuckDbLogStore {
    pub fn query_logs(
        &self,
        limit: u32,
        offset: u32,
        domain_filter: Option<&str>,
        action_filter: Option<&str>,
    ) -> anyhow::Result<LogQueryResult> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("duckdb mutex poisoned"))?;

        let mut where_clauses = Vec::new();
        if let Some(d) = domain_filter {
            where_clauses.push(format!("query_domain LIKE '%{}%'", d.replace('\'', "''")));
        }
        if let Some(a) = action_filter {
            where_clauses.push(format!("action = '{}'", a.replace('\'', "''")));
        }

        let where_sql = if where_clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", where_clauses.join(" AND "))
        };

        let count_sql = format!("SELECT COUNT(*) FROM dns_logs {}", where_sql);
        let total: i64 = guard
            .query_row(&count_sql, [], |row| row.get(0))
            .unwrap_or(0);

        let query_sql = format!(
            "SELECT timestamp, client_id, query_domain, action, protocol, response_time_ms \
             FROM dns_logs {} ORDER BY timestamp DESC LIMIT {} OFFSET {}",
            where_sql, limit, offset
        );

        let mut stmt = guard.prepare(&query_sql)?;
        let rows = stmt.query_map([], |row| {
            Ok(LogEntry {
                timestamp: row.get::<_, String>(0)?,
                client_id: row.get::<_, String>(1)?,
                query_domain: row.get::<_, String>(2)?,
                action: row.get::<_, String>(3)?,
                protocol: row.get::<_, String>(4)?,
                response_time_ms: row.get::<_, i64>(5)?,
            })
        })?;

        let mut logs = Vec::new();
        for row in rows {
            logs.push(row?);
        }

        Ok(LogQueryResult {
            logs,
            total: total as usize,
        })
    }

    pub fn query_stats(&self) -> anyhow::Result<QueryStats> {
        let guard = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("duckdb mutex poisoned"))?;

        let total: i64 = guard
            .query_row("SELECT COUNT(*) FROM dns_logs", [], |r| r.get(0))
            .unwrap_or(0);
        let blocked: i64 = guard
            .query_row(
                "SELECT COUNT(*) FROM dns_logs WHERE action='blocked'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        let mut top_domains_stmt = guard.prepare(
            "SELECT query_domain, COUNT(*) as cnt FROM dns_logs GROUP BY query_domain ORDER BY cnt DESC LIMIT 10",
        )?;
        let top_domains: Vec<(String, i64)> = top_domains_stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut top_blocked_stmt = guard.prepare(
            "SELECT query_domain, COUNT(*) as cnt FROM dns_logs WHERE action='blocked' GROUP BY query_domain ORDER BY cnt DESC LIMIT 10",
        )?;
        let top_blocked: Vec<(String, i64)> = top_blocked_stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut top_clients_stmt = guard.prepare(
            "SELECT client_id, COUNT(*) as cnt FROM dns_logs GROUP BY client_id ORDER BY cnt DESC LIMIT 10",
        )?;
        let top_clients: Vec<(String, i64)> = top_clients_stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(QueryStats {
            total_queries: total,
            blocked_queries: blocked,
            top_domains,
            top_blocked,
            top_clients,
        })
    }
}

/// Query logs from BOTH the RAM ring and DuckDB, with RAM records first (newest).
pub fn query_logs_merged(
    ring: &RamLogRing,
    duckdb: &DuckDbLogStore,
    limit: u32,
    offset: u32,
    domain_filter: Option<&str>,
    action_filter: Option<&str>,
) -> anyhow::Result<LogQueryResult> {
    // Get RAM records (newest first), filtered
    let ram_snapshot = ring.snapshot();
    let ram_entries: Vec<LogEntry> = ram_snapshot
        .iter()
        .rev()
        .filter(|r| {
            if let Some(d) = domain_filter {
                if !r.query_domain.contains(d) {
                    return false;
                }
            }
            if let Some(a) = action_filter {
                if r.action.as_str() != a {
                    return false;
                }
            }
            true
        })
        .map(|r| LogEntry {
            timestamp: r.timestamp.to_rfc3339(),
            client_id: r.client_id.clone(),
            query_domain: r.query_domain.clone(),
            action: r.action.as_str().to_string(),
            protocol: r.protocol.as_str().to_string(),
            response_time_ms: r.response_time_ms as i64,
        })
        .collect();

    let ram_total = ram_entries.len();

    // Get DuckDB results
    let disk_result =
        duckdb.query_logs(limit, offset.saturating_sub(ram_total as u32), domain_filter, action_filter)?;

    let combined_total = ram_total + disk_result.total;

    // Merge: RAM first (recent), then disk
    let offset = offset as usize;
    let limit = limit as usize;

    let mut merged = Vec::with_capacity(limit);
    if offset < ram_total {
        let ram_slice = &ram_entries[offset..ram_total.min(offset + limit)];
        merged.extend_from_slice(ram_slice);
    }

    if merged.len() < limit {
        let remaining = limit - merged.len();
        let disk_skip = offset.saturating_sub(ram_total);
        for entry in disk_result.logs.into_iter().skip(disk_skip).take(remaining) {
            merged.push(entry);
        }
    }

    Ok(LogQueryResult {
        logs: merged,
        total: combined_total,
    })
}

/// Compute stats from BOTH RAM ring and DuckDB.
pub fn query_stats_merged(
    ring: &RamLogRing,
    duckdb: &DuckDbLogStore,
) -> anyhow::Result<QueryStats> {
    let ram_snapshot = ring.snapshot();
    let disk_stats = duckdb.query_stats()?;

    let ram_total = ram_snapshot.len() as i64;
    let ram_blocked = ram_snapshot
        .iter()
        .filter(|r| r.action == sentinel_types::DnsAction::Blocked)
        .count() as i64;

    // Merge domain/client frequency maps
    let mut domain_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut blocked_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut client_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    // Seed with disk data
    for (d, c) in &disk_stats.top_domains {
        *domain_counts.entry(d.clone()).or_default() += c;
    }
    for (d, c) in &disk_stats.top_blocked {
        *blocked_counts.entry(d.clone()).or_default() += c;
    }
    for (cl, c) in &disk_stats.top_clients {
        *client_counts.entry(cl.clone()).or_default() += c;
    }

    // Add RAM data
    for r in &ram_snapshot {
        *domain_counts.entry(r.query_domain.clone()).or_default() += 1;
        *client_counts.entry(r.client_id.clone()).or_default() += 1;
        if r.action == sentinel_types::DnsAction::Blocked {
            *blocked_counts.entry(r.query_domain.clone()).or_default() += 1;
        }
    }

    let top_n = |map: std::collections::HashMap<String, i64>| -> Vec<(String, i64)> {
        let mut v: Vec<_> = map.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.truncate(10);
        v
    };

    Ok(QueryStats {
        total_queries: disk_stats.total_queries + ram_total,
        blocked_queries: disk_stats.blocked_queries + ram_blocked,
        top_domains: top_n(domain_counts),
        top_blocked: top_n(blocked_counts),
        top_clients: top_n(client_counts),
    })
}

// ─── SQLite config store ───

pub struct SqliteConfigStore {
    conn: Mutex<SqliteConnection>,
}

impl SqliteConfigStore {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = SqliteConnection::open(path).context("failed to open sqlite")?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS device_policies (
              id TEXT PRIMARY KEY,
              mac_address TEXT UNIQUE NOT NULL,
              group_memberships_json TEXT NOT NULL,
              wireguard_enabled INTEGER NOT NULL DEFAULT 0,
              risk_policy_mode TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS adlists (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              url TEXT UNIQUE NOT NULL,
              name TEXT NOT NULL DEFAULT '',
              kind TEXT NOT NULL DEFAULT 'block',
              enabled INTEGER NOT NULL DEFAULT 1,
              domain_count INTEGER NOT NULL DEFAULT 0,
              last_updated TEXT,
              last_status TEXT
            );

            CREATE TABLE IF NOT EXISTS domain_rules (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              kind TEXT NOT NULL,
              value TEXT NOT NULL,
              enabled INTEGER NOT NULL DEFAULT 1,
              comment TEXT,
              UNIQUE(kind, value)
            );

            CREATE TABLE IF NOT EXISTS groups (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT UNIQUE NOT NULL,
              description TEXT,
              enabled INTEGER NOT NULL DEFAULT 1
            );
            INSERT OR IGNORE INTO groups (id, name, description, enabled)
              VALUES (0, 'Default', 'Default group for all clients', 1);

            CREATE TABLE IF NOT EXISTS clients (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              ip TEXT UNIQUE NOT NULL,
              name TEXT
            );

            CREATE TABLE IF NOT EXISTS client_groups (
              client_id INTEGER NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
              group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
              PRIMARY KEY (client_id, group_id)
            );

            CREATE TABLE IF NOT EXISTS group_adlists (
              group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
              adlist_id INTEGER NOT NULL REFERENCES adlists(id) ON DELETE CASCADE,
              PRIMARY KEY (group_id, adlist_id)
            );

            CREATE TABLE IF NOT EXISTS group_rules (
              group_id INTEGER NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
              rule_id INTEGER NOT NULL REFERENCES domain_rules(id) ON DELETE CASCADE,
              PRIMARY KEY (group_id, rule_id)
            );
            "#,
        )
        .context("failed to initialize sqlite schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock_conn(&self) -> anyhow::Result<std::sync::MutexGuard<'_, SqliteConnection>> {
        self.conn
            .lock()
            .map_err(|_| anyhow::anyhow!("sqlite mutex poisoned"))
    }
}

// ─── ConfigStore (device policies) ───

impl ConfigStore for SqliteConfigStore {
    fn upsert_device_policy(&self, policy: DevicePolicy) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            r#"
            INSERT INTO device_policies (id, mac_address, group_memberships_json, wireguard_enabled, risk_policy_mode)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
              mac_address=excluded.mac_address,
              group_memberships_json=excluded.group_memberships_json,
              wireguard_enabled=excluded.wireguard_enabled,
              risk_policy_mode=excluded.risk_policy_mode
            "#,
            sqlite_params![
                policy.id.to_string(),
                policy.mac_address,
                serde_json::to_string(&policy.group_memberships)?,
                if policy.wireguard_enabled { 1 } else { 0 },
                risk_policy_mode_to_str(&policy.risk_policy_mode),
            ],
        )?;
        Ok(())
    }

    fn get_device_policy(&self, device_id: Uuid) -> anyhow::Result<Option<DevicePolicy>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard.prepare(
            "SELECT id, mac_address, group_memberships_json, wireguard_enabled, risk_policy_mode FROM device_policies WHERE id = ?1",
        )?;
        let mut rows = stmt.query(sqlite_params![device_id.to_string()])?;
        if let Some(row) = rows.next()? {
            return Ok(Some(row_to_device_policy(row)?));
        }
        Ok(None)
    }

    fn list_device_policies(&self) -> anyhow::Result<Vec<DevicePolicy>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard.prepare(
            "SELECT id, mac_address, group_memberships_json, wireguard_enabled, risk_policy_mode FROM device_policies",
        )?;
        let mapped = stmt.query_map([], |row| {
            let id = Uuid::parse_str(row.get::<_, String>(0)?.as_str())
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let mac = row.get::<_, String>(1)?;
            let groups_json = row.get::<_, String>(2)?;
            let groups = serde_json::from_str::<Vec<String>>(&groups_json)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let wireguard_enabled = row.get::<_, i64>(3)? == 1;
            let mode = match row.get::<_, String>(4)?.as_str() {
                "block" => RiskPolicyMode::Block,
                "bypass" => RiskPolicyMode::Bypass,
                _ => return Err(rusqlite::Error::InvalidQuery),
            };
            Ok(DevicePolicy {
                id,
                mac_address: mac,
                group_memberships: groups,
                wireguard_enabled,
                risk_policy_mode: mode,
            })
        })?;
        let mut output = Vec::new();
        for item in mapped {
            output.push(item?);
        }
        Ok(output)
    }
}

// ─── AdlistStore ───

impl AdlistStore for SqliteConfigStore {
    fn create_adlist(&self, url: &str, name: &str, kind: AdlistKind) -> anyhow::Result<Adlist> {
        let guard = self.lock_conn()?;
        let kind_str = match kind {
            AdlistKind::Block => "block",
            AdlistKind::Allow => "allow",
        };
        guard.execute(
            "INSERT INTO adlists (url, name, kind) VALUES (?1, ?2, ?3)",
            sqlite_params![url, name, kind_str],
        )?;
        let id = guard.last_insert_rowid();
        Ok(Adlist {
            id,
            url: url.to_string(),
            name: name.to_string(),
            kind,
            enabled: true,
            domain_count: 0,
            last_updated: None,
            last_status: None,
        })
    }

    fn list_adlists(&self) -> anyhow::Result<Vec<Adlist>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard.prepare(
            "SELECT id, url, name, kind, enabled, domain_count, last_updated, last_status FROM adlists ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Adlist {
                id: row.get(0)?,
                url: row.get(1)?,
                name: row.get(2)?,
                kind: match row.get::<_, String>(3)?.as_str() {
                    "allow" => AdlistKind::Allow,
                    _ => AdlistKind::Block,
                },
                enabled: row.get::<_, i64>(4)? == 1,
                domain_count: row.get(5)?,
                last_updated: row
                    .get::<_, Option<String>>(6)?
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc)),
                last_status: row.get(7)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn update_adlist_status(
        &self,
        id: i64,
        domain_count: i64,
        status: &str,
    ) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "UPDATE adlists SET domain_count=?1, last_updated=?2, last_status=?3 WHERE id=?4",
            sqlite_params![domain_count, Utc::now().to_rfc3339(), status, id],
        )?;
        Ok(())
    }

    fn delete_adlist(&self, id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute("DELETE FROM adlists WHERE id=?1", sqlite_params![id])?;
        Ok(())
    }

    fn toggle_adlist(&self, id: i64, enabled: bool) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "UPDATE adlists SET enabled=?1 WHERE id=?2",
            sqlite_params![if enabled { 1 } else { 0 }, id],
        )?;
        Ok(())
    }
}

// ─── DomainRuleStore ───

impl DomainRuleStore for SqliteConfigStore {
    fn create_domain_rule(
        &self,
        kind: DomainRuleKind,
        value: &str,
        comment: Option<&str>,
    ) -> anyhow::Result<DomainRule> {
        let guard = self.lock_conn()?;
        let kind_str = domain_rule_kind_to_str(kind);
        guard.execute(
            "INSERT INTO domain_rules (kind, value, comment) VALUES (?1, ?2, ?3)",
            sqlite_params![kind_str, value, comment],
        )?;
        let id = guard.last_insert_rowid();
        Ok(DomainRule {
            id,
            kind,
            value: value.to_string(),
            enabled: true,
            comment: comment.map(|s| s.to_string()),
        })
    }

    fn list_domain_rules(&self) -> anyhow::Result<Vec<DomainRule>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard
            .prepare("SELECT id, kind, value, enabled, comment FROM domain_rules ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(DomainRule {
                id: row.get(0)?,
                kind: domain_rule_kind_from_str(&row.get::<_, String>(1)?),
                value: row.get(2)?,
                enabled: row.get::<_, i64>(3)? == 1,
                comment: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn delete_domain_rule(&self, id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute("DELETE FROM domain_rules WHERE id=?1", sqlite_params![id])?;
        Ok(())
    }

    fn toggle_domain_rule(&self, id: i64, enabled: bool) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "UPDATE domain_rules SET enabled=?1 WHERE id=?2",
            sqlite_params![if enabled { 1 } else { 0 }, id],
        )?;
        Ok(())
    }
}

// ─── GroupStore ───

impl GroupStore for SqliteConfigStore {
    fn create_group(&self, name: &str, description: Option<&str>) -> anyhow::Result<Group> {
        let guard = self.lock_conn()?;
        guard.execute(
            "INSERT INTO groups (name, description) VALUES (?1, ?2)",
            sqlite_params![name, description],
        )?;
        let id = guard.last_insert_rowid();
        Ok(Group {
            id,
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            enabled: true,
        })
    }

    fn list_groups(&self) -> anyhow::Result<Vec<Group>> {
        let guard = self.lock_conn()?;
        let mut stmt =
            guard.prepare("SELECT id, name, description, enabled FROM groups ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(Group {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                enabled: row.get::<_, i64>(3)? == 1,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    fn delete_group(&self, id: i64) -> anyhow::Result<()> {
        if id == 0 {
            anyhow::bail!("cannot delete the default group");
        }
        let guard = self.lock_conn()?;
        guard.execute("DELETE FROM groups WHERE id=?1", sqlite_params![id])?;
        Ok(())
    }

    fn assign_adlist_to_group(&self, adlist_id: i64, group_id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "INSERT OR IGNORE INTO group_adlists (group_id, adlist_id) VALUES (?1, ?2)",
            sqlite_params![group_id, adlist_id],
        )?;
        Ok(())
    }

    fn remove_adlist_from_group(&self, adlist_id: i64, group_id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "DELETE FROM group_adlists WHERE group_id=?1 AND adlist_id=?2",
            sqlite_params![group_id, adlist_id],
        )?;
        Ok(())
    }

    fn assign_rule_to_group(&self, rule_id: i64, group_id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "INSERT OR IGNORE INTO group_rules (group_id, rule_id) VALUES (?1, ?2)",
            sqlite_params![group_id, rule_id],
        )?;
        Ok(())
    }

    fn remove_rule_from_group(&self, rule_id: i64, group_id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute(
            "DELETE FROM group_rules WHERE group_id=?1 AND rule_id=?2",
            sqlite_params![group_id, rule_id],
        )?;
        Ok(())
    }
}

// ─── ClientStore ───

impl ClientStore for SqliteConfigStore {
    fn upsert_client(
        &self,
        ip: &str,
        name: Option<&str>,
        group_ids: &[i64],
    ) -> anyhow::Result<Client> {
        let guard = self.lock_conn()?;
        guard.execute(
            "INSERT INTO clients (ip, name) VALUES (?1, ?2) ON CONFLICT(ip) DO UPDATE SET name=excluded.name",
            sqlite_params![ip, name],
        )?;
        let id: i64 = guard.query_row(
            "SELECT id FROM clients WHERE ip=?1",
            sqlite_params![ip],
            |r| r.get(0),
        )?;
        guard.execute(
            "DELETE FROM client_groups WHERE client_id=?1",
            sqlite_params![id],
        )?;
        for gid in group_ids {
            guard.execute(
                "INSERT OR IGNORE INTO client_groups (client_id, group_id) VALUES (?1, ?2)",
                sqlite_params![id, gid],
            )?;
        }
        Ok(Client {
            id,
            ip: ip.to_string(),
            name: name.map(|s| s.to_string()),
            group_ids: group_ids.to_vec(),
        })
    }

    fn list_clients(&self) -> anyhow::Result<Vec<Client>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard.prepare("SELECT id, ip, name FROM clients ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut clients = Vec::new();
        for row in rows {
            let (id, ip, name) = row?;
            let mut gstmt = guard
                .prepare("SELECT group_id FROM client_groups WHERE client_id=?1")?;
            let gids: Vec<i64> = gstmt
                .query_map(sqlite_params![id], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            clients.push(Client {
                id,
                ip,
                name,
                group_ids: gids,
            });
        }
        Ok(clients)
    }

    fn delete_client(&self, id: i64) -> anyhow::Result<()> {
        let guard = self.lock_conn()?;
        guard.execute("DELETE FROM clients WHERE id=?1", sqlite_params![id])?;
        Ok(())
    }

    fn get_client_by_ip(&self, ip: &str) -> anyhow::Result<Option<Client>> {
        let guard = self.lock_conn()?;
        let mut stmt = guard.prepare("SELECT id, ip, name FROM clients WHERE ip=?1")?;
        let mut rows = stmt.query(sqlite_params![ip])?;
        if let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let ip: String = row.get(1)?;
            let name: Option<String> = row.get(2)?;
            drop(rows);
            let mut gstmt = guard.prepare("SELECT group_id FROM client_groups WHERE client_id=?1")?;
            let gids: Vec<i64> = gstmt
                .query_map(sqlite_params![id], |r| r.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            return Ok(Some(Client {
                id,
                ip,
                name,
                group_ids: gids,
            }));
        }
        Ok(None)
    }
}

// ─── DTOs ───

#[derive(Debug, Clone, serde::Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub client_id: String,
    pub query_domain: String,
    pub action: String,
    pub protocol: String,
    pub response_time_ms: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct LogQueryResult {
    pub logs: Vec<LogEntry>,
    pub total: usize,
}

#[derive(Debug, serde::Serialize)]
pub struct QueryStats {
    pub total_queries: i64,
    pub blocked_queries: i64,
    pub top_domains: Vec<(String, i64)>,
    pub top_blocked: Vec<(String, i64)>,
    pub top_clients: Vec<(String, i64)>,
}

// ─── Helpers ───

fn row_to_device_policy(row: &rusqlite::Row) -> anyhow::Result<DevicePolicy> {
    let id = Uuid::parse_str(row.get::<_, String>(0)?.as_str())?;
    let mac = row.get::<_, String>(1)?;
    let groups_json = row.get::<_, String>(2)?;
    let groups = serde_json::from_str::<Vec<String>>(&groups_json)?;
    let wireguard_enabled = row.get::<_, i64>(3)? == 1;
    let mode = risk_policy_mode_from_str(row.get::<_, String>(4)?.as_str())?;
    Ok(DevicePolicy {
        id,
        mac_address: mac,
        group_memberships: groups,
        wireguard_enabled,
        risk_policy_mode: mode,
    })
}

fn risk_policy_mode_to_str(mode: &RiskPolicyMode) -> &'static str {
    match mode {
        RiskPolicyMode::Block => "block",
        RiskPolicyMode::Bypass => "bypass",
    }
}

fn risk_policy_mode_from_str(value: &str) -> anyhow::Result<RiskPolicyMode> {
    match value {
        "block" => Ok(RiskPolicyMode::Block),
        "bypass" => Ok(RiskPolicyMode::Bypass),
        _ => Err(anyhow::anyhow!("unknown risk policy mode: {value}")),
    }
}

fn domain_rule_kind_to_str(kind: DomainRuleKind) -> &'static str {
    match kind {
        DomainRuleKind::ExactDeny => "exact_deny",
        DomainRuleKind::ExactAllow => "exact_allow",
        DomainRuleKind::RegexDeny => "regex_deny",
        DomainRuleKind::RegexAllow => "regex_allow",
    }
}

fn domain_rule_kind_from_str(value: &str) -> DomainRuleKind {
    match value {
        "exact_deny" => DomainRuleKind::ExactDeny,
        "exact_allow" => DomainRuleKind::ExactAllow,
        "regex_deny" => DomainRuleKind::RegexDeny,
        "regex_allow" => DomainRuleKind::RegexAllow,
        _ => DomainRuleKind::ExactDeny,
    }
}

pub fn parse_rfc3339(ts: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(ts)?.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sentinel_types::{DnsAction, DnsProtocol};
    use std::path::PathBuf;

    fn temp_file(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("sentinel-{}-{}.db", name, Uuid::new_v4()));
        p
    }

    #[test]
    fn sqlite_roundtrip_device_policy() {
        let db = temp_file("sqlite-config");
        let store = SqliteConfigStore::new(&db).expect("sqlite should initialize");
        let policy = DevicePolicy {
            id: Uuid::new_v4(),
            mac_address: "AA:BB:CC:DD:EE:FF".to_string(),
            group_memberships: vec!["kids".to_string()],
            wireguard_enabled: true,
            risk_policy_mode: RiskPolicyMode::Block,
        };
        store
            .upsert_device_policy(policy.clone())
            .expect("upsert should succeed");
        let fetched = store
            .get_device_policy(policy.id)
            .expect("get should succeed")
            .expect("policy should exist");
        assert_eq!(fetched.mac_address, "AA:BB:CC:DD:EE:FF");
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn duckdb_accepts_log_batch() {
        let db = temp_file("duckdb-logs");
        let store = DuckDbLogStore::new(&db).expect("duckdb should initialize");
        let row = DnsLogRecord {
            timestamp: Utc::now(),
            client_id: "client-1".to_string(),
            query_domain: "example.com".to_string(),
            cname_chain: vec!["example.com".to_string()],
            action: DnsAction::Allowed,
            protocol: DnsProtocol::Udp,
            response_time_ms: 2,
        };
        store
            .append_dns_logs(&[row])
            .expect("duckdb insert should succeed");
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn ram_ring_circular_eviction() {
        let ring = RamLogRing::new(3);
        for i in 0..5 {
            ring.push(DnsLogRecord {
                timestamp: Utc::now(),
                client_id: format!("c-{i}"),
                query_domain: format!("d{i}.com"),
                cname_chain: vec![],
                action: DnsAction::Allowed,
                protocol: DnsProtocol::Udp,
                response_time_ms: 1,
            });
        }
        assert_eq!(ring.len(), 3);
        let snap = ring.snapshot();
        assert_eq!(snap[0].client_id, "c-2");
        assert_eq!(snap[2].client_id, "c-4");
        assert_eq!(ring.total_ingested(), 5);
    }

    #[test]
    fn ram_ring_drain_for_checkpoint() {
        let ring = RamLogRing::new(100);
        for i in 0..10 {
            ring.push(DnsLogRecord {
                timestamp: Utc::now(),
                client_id: format!("c-{i}"),
                query_domain: "test.com".to_string(),
                cname_chain: vec![],
                action: DnsAction::Allowed,
                protocol: DnsProtocol::Udp,
                response_time_ms: 1,
            });
        }
        let drained = ring.drain_for_checkpoint(5);
        assert_eq!(drained.len(), 5);
        assert_eq!(ring.len(), 5);
        assert_eq!(ring.total_checkpointed(), 5);
    }

    #[test]
    fn ram_ring_drain_all() {
        let ring = RamLogRing::new(100);
        for i in 0..7 {
            ring.push(DnsLogRecord {
                timestamp: Utc::now(),
                client_id: format!("c-{i}"),
                query_domain: "test.com".to_string(),
                cname_chain: vec![],
                action: DnsAction::Blocked,
                protocol: DnsProtocol::Udp,
                response_time_ms: 1,
            });
        }
        let drained = ring.drain_all();
        assert_eq!(drained.len(), 7);
        assert!(ring.is_empty());
    }

    #[test]
    fn adlist_crud() {
        let db = temp_file("sqlite-adlist");
        let store = SqliteConfigStore::new(&db).expect("sqlite init");
        let list = store
            .create_adlist("https://example.com/hosts", "test list", AdlistKind::Block)
            .expect("create");
        assert_eq!(list.url, "https://example.com/hosts");

        let lists = store.list_adlists().expect("list");
        assert!(lists.iter().any(|l| l.url == "https://example.com/hosts"));

        store
            .toggle_adlist(list.id, false)
            .expect("toggle");
        let lists2 = store.list_adlists().expect("list2");
        assert!(!lists2.iter().find(|l| l.id == list.id).unwrap().enabled);

        store.delete_adlist(list.id).expect("delete");
        let lists3 = store.list_adlists().expect("list3");
        assert!(!lists3.iter().any(|l| l.id == list.id));
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn domain_rule_crud() {
        let db = temp_file("sqlite-rules");
        let store = SqliteConfigStore::new(&db).expect("sqlite init");
        let rule = store
            .create_domain_rule(DomainRuleKind::ExactDeny, "ads.example.com", Some("ad server"))
            .expect("create");
        assert_eq!(rule.value, "ads.example.com");
        let rules = store.list_domain_rules().expect("list");
        assert!(rules.iter().any(|r| r.value == "ads.example.com"));
        store.delete_domain_rule(rule.id).expect("delete");
        let _ = std::fs::remove_file(db);
    }

    #[test]
    fn group_and_client_crud() {
        let db = temp_file("sqlite-groups");
        let store = SqliteConfigStore::new(&db).expect("sqlite init");

        let groups = store.list_groups().expect("list groups");
        assert!(groups.iter().any(|g| g.name == "Default"));

        let g = store
            .create_group("Kids", Some("Children's devices"))
            .expect("create group");

        let client = store
            .upsert_client("192.168.1.100", Some("Phone"), &[0, g.id])
            .expect("upsert client");
        assert_eq!(client.group_ids.len(), 2);

        let fetched = store
            .get_client_by_ip("192.168.1.100")
            .expect("get by ip")
            .expect("should exist");
        assert_eq!(fetched.name.as_deref(), Some("Phone"));

        store.delete_client(client.id).expect("delete client");
        store.delete_group(g.id).expect("delete group");
        let _ = std::fs::remove_file(db);
    }
}
