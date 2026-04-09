"use client";

import { useMemo } from "react";
import type {
  DevicePolicy,
  ResolveResponse,
  LogQueryResponse,
} from "@/lib/api";

interface DashboardShellProps {
  devices: DevicePolicy[];
  latestResolution: ResolveResponse | null;
  apiOnline: boolean;
  recentLogs: LogQueryResponse | null;
}

export function DashboardShell({
  devices,
  latestResolution,
  apiOnline,
  recentLogs,
}: DashboardShellProps) {
  const blockedCount = useMemo(
    () => devices.filter((x) => x.risk_policy_mode === "block").length,
    [devices]
  );

  const logBlockedCount = useMemo(
    () => recentLogs?.logs.filter((l) => l.action === "blocked").length ?? 0,
    [recentLogs]
  );

  return (
    <main className="mx-auto min-h-screen w-full max-w-7xl px-6 py-8 md:py-12">
      <header className="mb-8 flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <p className="text-xs uppercase tracking-[0.24em] text-slate-400">
            Sentinel DNS
          </p>
          <span
            className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium ${
              apiOnline
                ? "bg-emerald-500/10 text-emerald-400"
                : "bg-red-500/10 text-red-400"
            }`}
          >
            <span
              className={`h-1.5 w-1.5 rounded-full ${
                apiOnline ? "bg-emerald-400" : "bg-red-400"
              }`}
            />
            {apiOnline ? "Online" : "Offline"}
          </span>
        </div>
        <h1 className="font-display text-4xl leading-tight text-slate-100 md:text-6xl">
          Network control plane with cloaking-aware policy.
        </h1>
        <p className="max-w-3xl text-slate-300">
          Monitor CNAME chains, enforce per-device policy, and inspect query
          logs from one secure dashboard.
        </p>
      </header>

      {!apiOnline && (
        <div className="mb-6 rounded-xl border border-amber-500/20 bg-amber-500/5 px-4 py-3 text-sm text-amber-300">
          Cannot reach the Sentinel control-plane API. Make sure the backend is
          running and <code className="text-amber-200">SENTINEL_API_URL</code>{" "}
          is set correctly.
        </div>
      )}

      <section className="grid gap-4 md:grid-cols-4">
        <Card
          title="Managed devices"
          value={String(devices.length)}
          subtitle="Active policy mappings"
        />
        <Card
          title="Strict-mode clients"
          value={String(blockedCount)}
          subtitle="Risk mode set to block"
        />
        <Card
          title="Total queries"
          value={String(recentLogs?.total ?? 0)}
          subtitle="All logged DNS queries"
        />
        <Card
          title="Last response"
          value={
            latestResolution
              ? `${latestResolution.response_time_ms}ms`
              : "—"
          }
          subtitle="Sample query latency"
        />
      </section>

      <section className="mt-6 grid gap-6 lg:grid-cols-[2fr_1fr]">
        <article className="panel p-5">
          <h2 className="mb-4 text-lg font-semibold">CNAME Chain Inspector</h2>
          {latestResolution ? (
            <ol className="space-y-2">
              {latestResolution.cname_chain.map((hop, i) => (
                <li
                  key={`${hop}-${i}`}
                  className="rounded-lg border border-white/10 bg-black/20 px-3 py-2 font-mono text-sm text-pulse"
                >
                  {hop}
                </li>
              ))}
            </ol>
          ) : (
            <p className="text-sm text-slate-400">
              No resolution data available. Start the API and refresh.
            </p>
          )}
        </article>

        <div className="flex flex-col gap-6">
          <article className="panel p-5">
            <h2 className="mb-2 text-lg font-semibold">Block rate</h2>
            <p className="text-4xl font-bold text-signal">
              {recentLogs && recentLogs.logs.length > 0
                ? `${Math.round(
                    (logBlockedCount / recentLogs.logs.length) * 100
                  )}%`
                : "—"}
            </p>
            <p className="mt-1 text-sm text-slate-400">
              Of recent {recentLogs?.logs.length ?? 0} queries
            </p>
          </article>

          <article className="panel p-5">
            <h2 className="mb-2 text-lg font-semibold">Travel Mode</h2>
            <p className="text-sm text-slate-300">
              WireGuard profiles will be available in a future release.
            </p>
          </article>
        </div>
      </section>

      <section className="mt-6">
        <article className="panel p-5">
          <h2 className="mb-4 text-lg font-semibold">Recent Query Log</h2>
          {recentLogs && recentLogs.logs.length > 0 ? (
            <div className="overflow-x-auto">
              <table className="w-full text-left text-sm">
                <thead>
                  <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                    <th className="pb-2 pr-4">Time</th>
                    <th className="pb-2 pr-4">Domain</th>
                    <th className="pb-2 pr-4">Client</th>
                    <th className="pb-2 pr-4">Action</th>
                    <th className="pb-2 pr-4">Latency</th>
                  </tr>
                </thead>
                <tbody>
                  {recentLogs.logs.map((log, i) => (
                    <tr
                      key={`${log.timestamp}-${i}`}
                      className="border-b border-white/5"
                    >
                      <td className="py-2 pr-4 text-slate-400 tabular-nums">
                        {formatTimestamp(log.timestamp)}
                      </td>
                      <td className="py-2 pr-4 font-mono text-slate-200">
                        {log.query_domain}
                      </td>
                      <td className="py-2 pr-4 text-slate-400">
                        {log.client_id}
                      </td>
                      <td className="py-2 pr-4">
                        <span
                          className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${
                            log.action === "blocked"
                              ? "bg-red-500/10 text-red-400"
                              : log.action === "cached"
                                ? "bg-blue-500/10 text-blue-400"
                                : "bg-emerald-500/10 text-emerald-400"
                          }`}
                        >
                          {log.action}
                        </span>
                      </td>
                      <td className="py-2 pr-4 tabular-nums text-slate-400">
                        {log.response_time_ms}ms
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
            <p className="text-sm text-slate-400">
              No query logs yet. DNS queries will appear here once the backend
              is processing traffic.
            </p>
          )}
        </article>
      </section>
    </main>
  );
}

function Card({
  title,
  value,
  subtitle,
}: {
  title: string;
  value: string;
  subtitle: string;
}) {
  return (
    <article className="panel p-4">
      <p className="text-xs uppercase tracking-widest text-slate-400">
        {title}
      </p>
      <p className="mt-2 text-3xl font-semibold text-slate-100">{value}</p>
      <p className="mt-2 text-sm text-slate-300">{subtitle}</p>
    </article>
  );
}

function formatTimestamp(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return ts;
  }
}
