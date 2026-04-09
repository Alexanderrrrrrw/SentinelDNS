"use client";

import { useState } from "react";
import type {
  GravityStatus,
  QueryStats,
  ResolveResponse,
  LogQueryResponse,
} from "@/lib/api";
import { SetupWizard } from "@/components/setup-wizard";

interface Props {
  apiOnline: boolean;
  stats: QueryStats | null;
  gravityStatus: GravityStatus | null;
  latestResolution: ResolveResponse | null;
  recentLogs: LogQueryResponse | null;
}

export function DashboardClient({
  apiOnline,
  stats,
  gravityStatus,
  latestResolution,
  recentLogs,
}: Props) {
  const totalQueries = stats?.total_queries ?? 0;
  const blockedQueries = stats?.blocked_queries ?? 0;
  const blockRate =
    totalQueries > 0 ? Math.round((blockedQueries / totalQueries) * 100) : 0;
  const uniqueClients = stats?.top_clients?.length ?? 0;
  const latency = latestResolution?.response_time_ms ?? null;
  const bootstrapAgeLabel = formatAge(gravityStatus?.bootstrap_index_age_secs);
  const lastSyncLabel = formatLastSync(gravityStatus?.last_gravity_sync);

  const [wizardDismissed, setWizardDismissed] = useState(false);
  const showWizard = apiOnline && totalQueries === 0 && !wizardDismissed;

  return (
    <main className="mx-auto w-full max-w-7xl px-4 py-6 sm:px-6 md:py-10">
      {/* Setup wizard overlay */}
      {showWizard && (
        <SetupWizard onDismiss={() => setWizardDismissed(true)} />
      )}

      {/* Header */}
      <header className="mb-6 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-xl font-semibold text-slate-100 sm:text-2xl">
            Dashboard
          </h1>
          <StatusPill online={apiOnline} />
        </div>
        <p className="hidden text-xs text-slate-500 sm:block">
          Press{" "}
          <kbd className="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 font-mono text-[10px] text-slate-400">
            Ctrl+K
          </kbd>{" "}
          to search
        </p>
      </header>

      {!apiOnline && (
        <div className="mb-5 rounded-xl border border-amber-500/20 bg-amber-500/5 px-4 py-3 text-sm text-amber-300">
          Cannot reach the Sentinel control-plane API. Make sure the backend is
          running and <code className="text-amber-200">SENTINEL_API_URL</code>{" "}
          is set correctly.
        </div>
      )}

      {/* ── Bento grid ── */}
      <div className="bento-grid">
        {/* Block Rate — large card */}
        <div className="col-span-2 row-span-2 sm:col-span-1 lg:col-span-1">
          <GlassCard className="flex h-full flex-col justify-between" glow>
            <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
              Block Rate
            </p>
            <div className="my-3">
              <span className="text-5xl font-bold tabular-nums text-signal sm:text-6xl">
                {blockRate}
              </span>
              <span className="ml-1 text-xl text-signal/60">%</span>
            </div>
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-white/5">
              <div
                className="h-full rounded-full bg-signal/60 transition-all duration-700"
                style={{ width: `${blockRate}%` }}
              />
            </div>
            <p className="mt-2 text-[10px] text-slate-500">
              {blockedQueries.toLocaleString()} blocked of{" "}
              {totalQueries.toLocaleString()}
            </p>
          </GlassCard>
        </div>

        {/* Total Queries */}
        <GlassCard>
          <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
            Total Queries
          </p>
          <p className="mt-2 text-3xl font-bold tabular-nums text-slate-100">
            {totalQueries.toLocaleString()}
          </p>
          <p className="mt-1 text-[10px] text-slate-500">All time</p>
        </GlassCard>

        {/* Blocked */}
        <GlassCard>
          <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
            Blocked
          </p>
          <p className="mt-2 text-3xl font-bold tabular-nums text-red-400">
            {blockedQueries.toLocaleString()}
          </p>
          <p className="mt-1 text-[10px] text-slate-500">
            Threats neutralized
          </p>
        </GlassCard>

        {/* Unique Clients */}
        <GlassCard>
          <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
            Clients
          </p>
          <p className="mt-2 text-3xl font-bold tabular-nums text-pulse">
            {uniqueClients}
          </p>
          <p className="mt-1 text-[10px] text-slate-500">
            Unique devices seen
          </p>
        </GlassCard>

        {/* Latency */}
        <GlassCard>
          <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
            Latency
          </p>
          <p className="mt-2 text-3xl font-bold tabular-nums text-cyan-electric">
            {latency !== null ? `${latency}` : "—"}
            {latency !== null && (
              <span className="ml-0.5 text-sm font-normal text-cyan-electric/50">
                ms
              </span>
            )}
          </p>
          <p className="mt-1 text-[10px] text-slate-500">Sample query</p>
        </GlassCard>

        {/* Gravity freshness */}
        <GlassCard>
          <p className="text-[10px] font-medium uppercase tracking-[0.2em] text-slate-400">
            Gravity Freshness
          </p>
          <p className="mt-2 text-base font-semibold text-cyan-electric">
            {lastSyncLabel}
          </p>
          <p className="mt-1 text-[10px] text-slate-500">
            Bootstrap index age: {bootstrapAgeLabel}
          </p>
        </GlassCard>

        {/* Top Domains — spans 2 cols */}
        <div className="col-span-full lg:col-span-2">
          <GlassCard className="h-full">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-slate-300">
              Top Domains
            </h2>
            {stats?.top_domains && stats.top_domains.length > 0 ? (
              <div className="space-y-1.5">
                {stats.top_domains.slice(0, 8).map(([domain, count], i) => (
                  <DomainRow
                    key={domain}
                    rank={i + 1}
                    domain={domain}
                    count={count}
                    max={stats.top_domains[0][1]}
                  />
                ))}
              </div>
            ) : (
              <p className="py-4 text-center text-xs text-slate-500">
                No data yet
              </p>
            )}
          </GlassCard>
        </div>

        {/* Top Blocked — spans 2 cols */}
        <div className="col-span-full lg:col-span-2">
          <GlassCard className="h-full">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-red-300/80">
              Top Blocked
            </h2>
            {stats?.top_blocked && stats.top_blocked.length > 0 ? (
              <div className="space-y-1.5">
                {stats.top_blocked.slice(0, 8).map(([domain, count], i) => (
                  <DomainRow
                    key={domain}
                    rank={i + 1}
                    domain={domain}
                    count={count}
                    max={stats.top_blocked[0][1]}
                    variant="blocked"
                  />
                ))}
              </div>
            ) : (
              <p className="py-4 text-center text-xs text-slate-500">
                No blocked queries yet
              </p>
            )}
          </GlassCard>
        </div>

        {/* Top Clients */}
        <div className="col-span-full lg:col-span-2">
          <GlassCard className="h-full">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-slate-300">
              Top Clients
            </h2>
            {stats?.top_clients && stats.top_clients.length > 0 ? (
              <div className="space-y-1.5">
                {stats.top_clients.slice(0, 8).map(([client, count]) => (
                  <div
                    key={client}
                    className="flex items-center justify-between rounded-lg bg-white/[0.02] px-3 py-1.5"
                  >
                    <span className="truncate font-mono text-xs text-slate-300">
                      {client}
                    </span>
                    <span className="shrink-0 pl-3 font-mono text-xs tabular-nums text-slate-500">
                      {count}
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="py-4 text-center text-xs text-slate-500">
                No data yet
              </p>
            )}
          </GlassCard>
        </div>

        {/* CNAME Inspector */}
        <div className="col-span-full lg:col-span-2">
          <GlassCard className="h-full">
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-pulse/80">
              CNAME Chain Inspector
            </h2>
            {latestResolution &&
            latestResolution.cname_chain.length > 0 ? (
              <div className="space-y-1">
                {latestResolution.cname_chain.map((hop, i) => (
                  <div
                    key={`${hop}-${i}`}
                    className="flex items-center gap-2"
                  >
                    <span className="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-pulse/10 font-mono text-[9px] text-pulse/70">
                      {i + 1}
                    </span>
                    <span className="truncate font-mono text-xs text-slate-300">
                      {hop}
                    </span>
                  </div>
                ))}
              </div>
            ) : (
              <p className="py-4 text-center text-xs text-slate-500">
                No resolution data yet
              </p>
            )}
          </GlassCard>
        </div>

        {/* Recent Queries — full width */}
        <div className="col-span-full">
          <GlassCard>
            <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-slate-300">
              Recent Queries
            </h2>
            {recentLogs && recentLogs.logs.length > 0 ? (
              <div className="overflow-x-auto">
                <table className="w-full text-left text-xs">
                  <thead>
                    <tr className="border-b border-white/[0.06] text-[10px] uppercase tracking-wider text-slate-500">
                      <th className="pb-2 pr-3 font-medium">Time</th>
                      <th className="pb-2 pr-3 font-medium">Domain</th>
                      <th className="pb-2 pr-3 font-medium">Client</th>
                      <th className="pb-2 pr-3 font-medium">Action</th>
                      <th className="pb-2 font-medium">Latency</th>
                    </tr>
                  </thead>
                  <tbody>
                    {recentLogs.logs.map((log, i) => (
                      <tr
                        key={`${log.timestamp}-${i}`}
                        className="border-b border-white/[0.03]"
                      >
                        <td className="py-1.5 pr-3 tabular-nums text-slate-500">
                          {formatTs(log.timestamp)}
                        </td>
                        <td className="py-1.5 pr-3 font-mono text-slate-300">
                          {log.query_domain}
                        </td>
                        <td className="py-1.5 pr-3 text-slate-500">
                          {log.client_id}
                        </td>
                        <td className="py-1.5 pr-3">
                          <ActionBadge action={log.action} />
                        </td>
                        <td className="py-1.5 tabular-nums text-slate-500">
                          {log.response_time_ms}ms
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <p className="py-6 text-center text-xs text-slate-500">
                No query logs yet — DNS queries will appear here once traffic flows.
              </p>
            )}
          </GlassCard>
        </div>
      </div>
    </main>
  );
}

/* ── Subcomponents ── */

function GlassCard({
  children,
  className = "",
  glow = false,
}: {
  children: React.ReactNode;
  className?: string;
  glow?: boolean;
}) {
  return (
    <div className={`${glow ? "glass-glow" : "glass"} p-4 sm:p-5 ${className}`}>
      {children}
    </div>
  );
}

function StatusPill({ online }: { online: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[10px] font-medium ${
        online
          ? "bg-signal/10 text-signal"
          : "bg-red-500/10 text-red-400"
      }`}
    >
      <span
        className={`h-1.5 w-1.5 rounded-full ${
          online ? "animate-pulse bg-signal" : "bg-red-400"
        }`}
      />
      {online ? "Online" : "Offline"}
    </span>
  );
}

function DomainRow({
  rank,
  domain,
  count,
  max,
  variant = "default",
}: {
  rank: number;
  domain: string;
  count: number;
  max: number;
  variant?: "default" | "blocked";
}) {
  const pct = max > 0 ? (count / max) * 100 : 0;
  const barColor =
    variant === "blocked" ? "bg-red-500/20" : "bg-signal/15";
  const textColor =
    variant === "blocked" ? "text-red-300" : "text-slate-300";

  return (
    <div className="relative overflow-hidden rounded-lg bg-white/[0.02] px-3 py-1.5">
      <div
        className={`absolute inset-y-0 left-0 ${barColor} transition-all duration-500`}
        style={{ width: `${pct}%` }}
      />
      <div className="relative flex items-center justify-between">
        <div className="flex items-center gap-2 overflow-hidden">
          <span className="shrink-0 font-mono text-[9px] text-slate-600">
            {rank}
          </span>
          <span className={`truncate font-mono text-xs ${textColor}`}>
            {domain}
          </span>
        </div>
        <span className="shrink-0 pl-3 font-mono text-xs tabular-nums text-slate-500">
          {count.toLocaleString()}
        </span>
      </div>
    </div>
  );
}

function ActionBadge({ action }: { action: string }) {
  const color =
    action === "blocked"
      ? "bg-red-500/10 text-red-400"
      : action === "cached"
        ? "bg-blue-500/10 text-blue-400"
        : "bg-signal/10 text-signal";
  return (
    <span
      className={`inline-block rounded px-1.5 py-0.5 text-[10px] font-medium ${color}`}
    >
      {action}
    </span>
  );
}

function formatTs(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return ts;
  }
}

function formatAge(secs?: number | null): string {
  if (secs === null || secs === undefined) return "not available";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins} min`;
  const hours = Math.floor(mins / 60);
  if (hours < 48) return `${hours} h`;
  const days = Math.floor(hours / 24);
  return `${days} d`;
}

function formatLastSync(ts?: string | null): string {
  if (!ts) return "Never synced";
  const when = new Date(ts);
  if (Number.isNaN(when.getTime())) return ts;
  const deltaMs = Date.now() - when.getTime();
  if (deltaMs < 0) return when.toLocaleString();
  const mins = Math.floor(deltaMs / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 48) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return `${days} d ago`;
}
