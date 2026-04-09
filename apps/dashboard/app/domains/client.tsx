"use client";

import { useState } from "react";
import type { DomainRule } from "@/lib/api";

const KINDS = [
  { value: "exact_deny", label: "Exact Deny" },
  { value: "exact_allow", label: "Exact Allow" },
  { value: "regex_deny", label: "Regex Deny" },
  { value: "regex_allow", label: "Regex Allow" },
] as const;

export function DomainsClient({ initial }: { initial: DomainRule[] }) {
  const [rules, setRules] = useState(initial);
  const [kind, setKind] = useState<DomainRule["kind"]>("exact_deny");
  const [value, setValue] = useState("");
  const [comment, setComment] = useState("");

  async function handleAdd() {
    if (!value.trim()) return;
    const res = await fetch("/api/proxy/domains", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ kind, value, comment: comment || undefined }),
    });
    if (res.ok) {
      const rule = await res.json();
      setRules((prev) => [...prev, rule]);
      setValue("");
      setComment("");
    }
  }

  async function handleDelete(id: number) {
    await fetch(`/api/proxy/domains/${id}`, { method: "DELETE" });
    setRules((prev) => prev.filter((r) => r.id !== id));
  }

  async function handleToggle(id: number, enabled: boolean) {
    await fetch(`/api/proxy/domains/${id}/toggle`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: !enabled }),
    });
    setRules((prev) =>
      prev.map((r) => (r.id === id ? { ...r, enabled: !enabled } : r))
    );
  }

  return (
    <div className="space-y-6">
      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Add Rule</h2>
        <div className="flex flex-wrap gap-3">
          <select
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100"
            value={kind}
            onChange={(e) => setKind(e.target.value as DomainRule["kind"])}
          >
            {KINDS.map((k) => (
              <option key={k.value} value={k.value}>
                {k.label}
              </option>
            ))}
          </select>
          <input
            className="flex-1 min-w-[200px] rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder={kind.startsWith("regex") ? "^ads\\d+\\.example\\.com$" : "ads.example.com"}
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
          <input
            className="w-48 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="Comment (optional)"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
          />
          <button
            onClick={handleAdd}
            className="rounded-lg bg-signal/20 px-4 py-2 text-sm font-medium text-signal hover:bg-signal/30 transition-colors"
          >
            Add Rule
          </button>
        </div>
      </div>

      <div className="panel p-5">
        <h2 className="mb-4 text-lg font-semibold">Rules ({rules.length})</h2>
        {rules.length > 0 ? (
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                  <th className="pb-2 pr-4">Type</th>
                  <th className="pb-2 pr-4">Value</th>
                  <th className="pb-2 pr-4">Comment</th>
                  <th className="pb-2 pr-4">Status</th>
                  <th className="pb-2">Actions</th>
                </tr>
              </thead>
              <tbody>
                {rules.map((rule) => (
                  <tr key={rule.id} className={`border-b border-white/5 ${!rule.enabled ? "opacity-50" : ""}`}>
                    <td className="py-2 pr-4">
                      <KindBadge kind={rule.kind} />
                    </td>
                    <td className="py-2 pr-4 font-mono text-slate-200">{rule.value}</td>
                    <td className="py-2 pr-4 text-slate-400">{rule.comment ?? "—"}</td>
                    <td className="py-2 pr-4">
                      <span className={`text-xs ${rule.enabled ? "text-emerald-400" : "text-slate-500"}`}>
                        {rule.enabled ? "Active" : "Disabled"}
                      </span>
                    </td>
                    <td className="py-2">
                      <div className="flex gap-2">
                        <button
                          onClick={() => handleToggle(rule.id, rule.enabled)}
                          className="text-xs text-slate-400 hover:text-slate-200"
                        >
                          {rule.enabled ? "Disable" : "Enable"}
                        </button>
                        <button
                          onClick={() => handleDelete(rule.id)}
                          className="text-xs text-red-400 hover:text-red-300"
                        >
                          Delete
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-slate-400">No domain rules configured.</p>
        )}
      </div>
    </div>
  );
}

function KindBadge({ kind }: { kind: DomainRule["kind"] }) {
  const styles: Record<string, string> = {
    exact_deny: "bg-red-500/10 text-red-400",
    exact_allow: "bg-emerald-500/10 text-emerald-400",
    regex_deny: "bg-orange-500/10 text-orange-400",
    regex_allow: "bg-teal-500/10 text-teal-400",
  };
  const labels: Record<string, string> = {
    exact_deny: "Exact Deny",
    exact_allow: "Exact Allow",
    regex_deny: "Regex Deny",
    regex_allow: "Regex Allow",
  };
  return (
    <span className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${styles[kind]}`}>
      {labels[kind]}
    </span>
  );
}
