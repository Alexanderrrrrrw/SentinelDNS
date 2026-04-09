export type DnsAction = "allowed" | "blocked" | "cached";

export interface ResolveResponse {
  action: DnsAction;
  cname_chain: string[];
  response_time_ms: number;
}

export interface DevicePolicy {
  id: string;
  mac_address: string;
  group_memberships: string[];
  wireguard_enabled: boolean;
  risk_policy_mode: "block" | "bypass";
}

export interface LogEntry {
  timestamp: string;
  client_id: string;
  query_domain: string;
  action: string;
  protocol: string;
  response_time_ms: number;
}

export interface LogQueryResponse {
  logs: LogEntry[];
  total: number;
}

export interface Adlist {
  id: number;
  url: string;
  name: string;
  kind: "block" | "allow";
  enabled: boolean;
  domain_count: number;
  last_updated: string | null;
  last_status: string | null;
}

export interface DomainRule {
  id: number;
  kind: "exact_deny" | "exact_allow" | "regex_deny" | "regex_allow";
  value: string;
  enabled: boolean;
  comment: string | null;
}

export interface Group {
  id: number;
  name: string;
  description: string | null;
  enabled: boolean;
}

export interface Client {
  id: number;
  ip: string;
  name: string | null;
  group_ids: number[];
}

export interface QueryStats {
  total_queries: number;
  blocked_queries: number;
  top_domains: [string, number][];
  top_blocked: [string, number][];
  top_clients: [string, number][];
}

export interface GravityStatus {
  bootstrap_index_path: string;
  bootstrap_index_present: boolean;
  bootstrap_index_age_secs: number | null;
  last_gravity_sync: string | null;
}

export interface GravityResponse {
  message: string;
  lists_processed: number;
  total_block_domains: number;
  total_allow_domains: number;
}

function getBaseUrl(): string {
  return process.env.SENTINEL_API_URL ?? "http://127.0.0.1:8080";
}

function getAdminToken(): string {
  return process.env.SENTINEL_ADMIN_TOKEN ?? "";
}

function authHeaders(): Record<string, string> {
  const token = getAdminToken();
  if (token) {
    return { "x-admin-token": token };
  }
  return {};
}

// ─── Health ───

export async function fetchHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${getBaseUrl()}/healthz`, { cache: "no-store" });
    return res.ok;
  } catch {
    return false;
  }
}

// ─── Stats ───

export async function fetchStats(): Promise<QueryStats | null> {
  try {
    const res = await fetch(`${getBaseUrl()}/api/stats`, {
      cache: "no-store",
      headers: authHeaders(),
    });
    if (!res.ok) return null;
    return (await res.json()) as QueryStats;
  } catch {
    return null;
  }
}

export async function fetchGravityStatus(): Promise<GravityStatus | null> {
  try {
    const res = await fetch(`${getBaseUrl()}/api/gravity/status`, {
      cache: "no-store",
      headers: authHeaders(),
    });
    if (!res.ok) return null;
    return (await res.json()) as GravityStatus;
  } catch {
    return null;
  }
}

// ─── Devices ───

export async function fetchDevices(): Promise<DevicePolicy[]> {
  const res = await fetch(`${getBaseUrl()}/api/devices`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`Failed to fetch devices: ${res.status}`);
  return (await res.json()) as DevicePolicy[];
}

// ─── Resolve ───

export async function resolveSampleDomain(
  domain: string
): Promise<ResolveResponse> {
  const res = await fetch(`${getBaseUrl()}/api/resolve`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      client_id: "dashboard-preview",
      query_domain: domain,
      protocol: "udp",
    }),
    cache: "no-store",
  });
  if (!res.ok) throw new Error(`Failed to resolve domain: ${res.status}`);
  return (await res.json()) as ResolveResponse;
}

// ─── Logs ───

export async function fetchLogs(
  limit = 50,
  offset = 0,
  domain?: string,
  action?: string
): Promise<LogQueryResponse> {
  const params = new URLSearchParams();
  params.set("limit", String(limit));
  params.set("offset", String(offset));
  if (domain) params.set("domain", domain);
  if (action) params.set("action", action);

  const res = await fetch(`${getBaseUrl()}/api/logs?${params.toString()}`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`Failed to fetch logs: ${res.status}`);
  return (await res.json()) as LogQueryResponse;
}

// ─── Adlists ───

export async function fetchAdlists(): Promise<Adlist[]> {
  const res = await fetch(`${getBaseUrl()}/api/lists`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) return [];
  return (await res.json()) as Adlist[];
}

export async function createAdlist(
  url: string,
  name: string,
  kind: "block" | "allow"
): Promise<Adlist> {
  const res = await fetch(`${getBaseUrl()}/api/lists`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ url, name, kind }),
  });
  if (!res.ok) throw new Error(`Failed to create adlist: ${res.status}`);
  return (await res.json()) as Adlist;
}

export async function deleteAdlist(id: number): Promise<void> {
  await fetch(`${getBaseUrl()}/api/lists/${id}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
}

export async function toggleAdlist(
  id: number,
  enabled: boolean
): Promise<void> {
  await fetch(`${getBaseUrl()}/api/lists/${id}/toggle`, {
    method: "PUT",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ enabled }),
  });
}

export async function triggerGravity(): Promise<GravityResponse> {
  const res = await fetch(`${getBaseUrl()}/api/gravity/update`, {
    method: "POST",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`Failed to trigger gravity: ${res.status}`);
  return (await res.json()) as GravityResponse;
}

// ─── Domain rules ───

export async function fetchDomainRules(): Promise<DomainRule[]> {
  const res = await fetch(`${getBaseUrl()}/api/domains`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) return [];
  return (await res.json()) as DomainRule[];
}

export async function createDomainRule(
  kind: DomainRule["kind"],
  value: string,
  comment?: string
): Promise<DomainRule> {
  const res = await fetch(`${getBaseUrl()}/api/domains`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ kind, value, comment }),
  });
  if (!res.ok) throw new Error(`Failed to create rule: ${res.status}`);
  return (await res.json()) as DomainRule;
}

export async function deleteDomainRule(id: number): Promise<void> {
  await fetch(`${getBaseUrl()}/api/domains/${id}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
}

export async function toggleDomainRule(
  id: number,
  enabled: boolean
): Promise<void> {
  await fetch(`${getBaseUrl()}/api/domains/${id}/toggle`, {
    method: "PUT",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ enabled }),
  });
}

// ─── Groups ───

export async function fetchGroups(): Promise<Group[]> {
  const res = await fetch(`${getBaseUrl()}/api/groups`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) return [];
  return (await res.json()) as Group[];
}

export async function createGroup(
  name: string,
  description?: string
): Promise<Group> {
  const res = await fetch(`${getBaseUrl()}/api/groups`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ name, description }),
  });
  if (!res.ok) throw new Error(`Failed to create group: ${res.status}`);
  return (await res.json()) as Group;
}

export async function deleteGroup(id: number): Promise<void> {
  await fetch(`${getBaseUrl()}/api/groups/${id}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
}

// ─── Clients ───

export async function fetchClients(): Promise<Client[]> {
  const res = await fetch(`${getBaseUrl()}/api/clients`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) return [];
  return (await res.json()) as Client[];
}

export async function upsertClient(
  ip: string,
  name?: string,
  group_ids?: number[]
): Promise<Client> {
  const res = await fetch(`${getBaseUrl()}/api/clients`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ ip, name, group_ids }),
  });
  if (!res.ok) throw new Error(`Failed to upsert client: ${res.status}`);
  return (await res.json()) as Client;
}

export async function deleteClient(id: number): Promise<void> {
  await fetch(`${getBaseUrl()}/api/clients/${id}`, {
    method: "DELETE",
    headers: authHeaders(),
  });
}

// ─── Heuristic scoring ───

export interface HeuristicSignal {
  name: string;
  weight: number;
  detail: string;
}

export interface HeuristicScoreResponse {
  domain: string;
  score: number;
  verdict: "clean" | "warn" | "suspicious";
  signals: HeuristicSignal[];
  threshold_block: number;
  threshold_warn: number;
}

export interface HeuristicsStatus {
  heuristics_enabled: boolean;
  block_threshold: number;
  warn_threshold: number;
}

export async function scoreDomain(
  domain: string
): Promise<HeuristicScoreResponse> {
  const res = await fetch(`${getBaseUrl()}/api/heuristics/score`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ domain }),
  });
  if (!res.ok) throw new Error(`Failed to score domain: ${res.status}`);
  return (await res.json()) as HeuristicScoreResponse;
}

export async function toggleHeuristics(
  enabled: boolean
): Promise<void> {
  await fetch(`${getBaseUrl()}/api/heuristics/toggle`, {
    method: "PUT",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify({ enabled }),
  });
}

export async function fetchHeuristicsStatus(): Promise<HeuristicsStatus | null> {
  try {
    const res = await fetch(`${getBaseUrl()}/api/heuristics/status`, {
      cache: "no-store",
      headers: authHeaders(),
    });
    if (!res.ok) return null;
    return (await res.json()) as HeuristicsStatus;
  } catch {
    return null;
  }
}

// ─── Config export/import ───

export async function exportConfig(): Promise<object> {
  const res = await fetch(`${getBaseUrl()}/api/config/export`, {
    cache: "no-store",
    headers: authHeaders(),
  });
  if (!res.ok) throw new Error(`Failed to export config: ${res.status}`);
  return await res.json();
}

export async function importConfig(config: object): Promise<void> {
  const res = await fetch(`${getBaseUrl()}/api/config/import`, {
    method: "POST",
    headers: { "Content-Type": "application/json", ...authHeaders() },
    body: JSON.stringify(config),
  });
  if (!res.ok) throw new Error(`Failed to import config: ${res.status}`);
}
