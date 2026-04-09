"use client";

import { useState } from "react";
import type { LogQueryResponse } from "@/lib/api";

export function QueryLogClient({ initial }: { initial: LogQueryResponse | null }) {
  const [logs] = useState(initial);

  return (
    <div className="panel p-5">
      {logs && logs.logs.length > 0 ? (
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                <th className="pb-2 pr-4">Time</th>
                <th className="pb-2 pr-4">Domain</th>
                <th className="pb-2 pr-4">Client</th>
                <th className="pb-2 pr-4">Action</th>
                <th className="pb-2 pr-4">Protocol</th>
                <th className="pb-2 pr-4">Latency</th>
              </tr>
            </thead>
            <tbody>
              {logs.logs.map((log, i) => (
                <tr key={`${log.timestamp}-${i}`} className="border-b border-white/5">
                  <td className="py-2 pr-4 text-slate-400 tabular-nums">
                    {formatTs(log.timestamp)}
                  </td>
                  <td className="py-2 pr-4 font-mono text-slate-200">{log.query_domain}</td>
                  <td className="py-2 pr-4 text-slate-400">{log.client_id}</td>
                  <td className="py-2 pr-4">
                    <ActionBadge action={log.action} />
                  </td>
                  <td className="py-2 pr-4 text-slate-400 uppercase text-xs">{log.protocol}</td>
                  <td className="py-2 pr-4 tabular-nums text-slate-400">{log.response_time_ms}ms</td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="mt-3 text-xs text-slate-500">
            Showing {logs.logs.length} of {logs.total} total entries
          </p>
        </div>
      ) : (
        <p className="text-sm text-slate-400">No query logs yet.</p>
      )}
    </div>
  );
}

function ActionBadge({ action }: { action: string }) {
  const color =
    action === "blocked"
      ? "bg-red-500/10 text-red-400"
      : action === "cached"
        ? "bg-blue-500/10 text-blue-400"
        : "bg-emerald-500/10 text-emerald-400";
  return (
    <span className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${color}`}>
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
