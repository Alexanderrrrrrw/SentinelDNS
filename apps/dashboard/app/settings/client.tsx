"use client";

import { useState } from "react";

export function SettingsClient() {
  const [exportData, setExportData] = useState<string | null>(null);
  const [importData, setImportData] = useState("");
  const [msg, setMsg] = useState<string | null>(null);

  async function handleExport() {
    try {
      const res = await fetch("/api/proxy/config/export");
      if (res.ok) {
        const data = await res.json();
        setExportData(JSON.stringify(data, null, 2));
        setMsg("Configuration exported successfully.");
      } else {
        setMsg("Export failed.");
      }
    } catch {
      setMsg("Export failed.");
    }
  }

  async function handleImport() {
    if (!importData.trim()) return;
    try {
      const parsed = JSON.parse(importData);
      const res = await fetch("/api/proxy/config/import", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(parsed),
      });
      if (res.ok) {
        setMsg("Configuration imported successfully.");
        setImportData("");
      } else {
        setMsg("Import failed.");
      }
    } catch {
      setMsg("Invalid JSON.");
    }
  }

  return (
    <div className="space-y-6">
      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Configuration</h2>
        <p className="mb-4 text-sm text-slate-400">
          Export your entire Sentinel configuration as JSON, or import a previously exported config.
          This includes adlists, domain rules, groups, and clients.
        </p>

        <div className="flex gap-3 mb-4">
          <button
            onClick={handleExport}
            className="rounded-lg bg-pulse/20 px-4 py-2 text-sm font-medium text-pulse hover:bg-pulse/30 transition-colors"
          >
            Export Config
          </button>
          <button
            onClick={handleImport}
            className="rounded-lg bg-signal/20 px-4 py-2 text-sm font-medium text-signal hover:bg-signal/30 transition-colors"
          >
            Import Config
          </button>
        </div>

        {msg && <p className="mb-4 text-sm text-slate-300">{msg}</p>}
      </div>

      {exportData && (
        <div className="panel p-5">
          <h2 className="mb-3 text-lg font-semibold">Exported Configuration</h2>
          <pre className="max-h-96 overflow-auto rounded-lg bg-black/40 p-4 font-mono text-xs text-slate-300">
            {exportData}
          </pre>
        </div>
      )}

      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Import Configuration</h2>
        <textarea
          className="w-full h-48 rounded-lg border border-white/10 bg-white/5 px-4 py-3 font-mono text-xs text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
          placeholder='Paste JSON config here...'
          value={importData}
          onChange={(e) => setImportData(e.target.value)}
        />
      </div>

      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Environment Variables</h2>
        <div className="space-y-2 text-sm">
          <EnvRow name="SENTINEL_API_URL" desc="Backend API URL" />
          <EnvRow name="SENTINEL_ADMIN_TOKEN" desc="Admin authentication token" />
          <EnvRow name="SENTINEL_BLOCK_MODE" desc="nxdomain | null_ip" />
          <EnvRow name="SENTINEL_UPSTREAM" desc="tls://1.1.1.1:853#cloudflare-dns.com" />
          <EnvRow name="SENTINEL_DNS_BIND" desc="DNS listener bind address (default 0.0.0.0:5353)" />
          <EnvRow name="SENTINEL_GRAVITY_INTERVAL_SECS" desc="Auto gravity update interval (default 604800)" />
        </div>
      </div>
    </div>
  );
}

function EnvRow({ name, desc }: { name: string; desc: string }) {
  return (
    <div className="flex items-baseline gap-3">
      <code className="shrink-0 rounded bg-white/5 px-2 py-0.5 text-xs text-pulse">{name}</code>
      <span className="text-slate-400">{desc}</span>
    </div>
  );
}
