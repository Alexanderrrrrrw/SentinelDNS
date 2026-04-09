"use client";

import { useState, useTransition } from "react";
import type { Adlist } from "@/lib/api";

export function ListsClient({ initial }: { initial: Adlist[] }) {
  const [lists, setLists] = useState(initial);
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [kind, setKind] = useState<"block" | "allow">("block");
  const [isPending, startTransition] = useTransition();
  const [gravityMsg, setGravityMsg] = useState<string | null>(null);

  async function handleAdd() {
    if (!url.trim()) return;
    const res = await fetch("/api/proxy/lists", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ url, name, kind }),
    });
    if (res.ok) {
      const newList = await res.json();
      setLists((prev) => [...prev, newList]);
      setUrl("");
      setName("");
    }
  }

  async function handleDelete(id: number) {
    await fetch(`/api/proxy/lists/${id}`, { method: "DELETE" });
    setLists((prev) => prev.filter((l) => l.id !== id));
  }

  async function handleToggle(id: number, enabled: boolean) {
    await fetch(`/api/proxy/lists/${id}/toggle`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled: !enabled }),
    });
    setLists((prev) =>
      prev.map((l) => (l.id === id ? { ...l, enabled: !enabled } : l))
    );
  }

  async function handleGravity() {
    setGravityMsg("Updating gravity...");
    startTransition(async () => {
      const res = await fetch("/api/proxy/gravity/update", { method: "POST" });
      if (res.ok) {
        const data = await res.json();
        setGravityMsg(
          `Updated: ${data.total_block_domains} block + ${data.total_allow_domains} allow domains from ${data.lists_processed} lists`
        );
      } else {
        setGravityMsg("Gravity update failed.");
      }
    });
  }

  return (
    <div className="space-y-6">
      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Add Adlist</h2>
        <div className="flex flex-wrap gap-3">
          <input
            className="flex-1 min-w-[200px] rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="https://example.com/hosts.txt"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
          <input
            className="w-40 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="Name (optional)"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <select
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100"
            value={kind}
            onChange={(e) => setKind(e.target.value as "block" | "allow")}
          >
            <option value="block">Block</option>
            <option value="allow">Allow</option>
          </select>
          <button
            onClick={handleAdd}
            className="rounded-lg bg-signal/20 px-4 py-2 text-sm font-medium text-signal hover:bg-signal/30 transition-colors"
          >
            Add List
          </button>
        </div>
      </div>

      <div className="panel p-5">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Gravity</h2>
          <button
            onClick={handleGravity}
            disabled={isPending}
            className="rounded-lg bg-pulse/20 px-4 py-2 text-sm font-medium text-pulse hover:bg-pulse/30 transition-colors disabled:opacity-50"
          >
            {isPending ? "Updating..." : "Update Gravity"}
          </button>
        </div>
        {gravityMsg && <p className="text-sm text-slate-300">{gravityMsg}</p>}
      </div>

      <div className="panel p-5">
        <h2 className="mb-4 text-lg font-semibold">Configured Lists ({lists.length})</h2>
        {lists.length > 0 ? (
          <div className="space-y-3">
            {lists.map((list) => (
              <div
                key={list.id}
                className={`flex items-center justify-between rounded-lg border px-4 py-3 ${
                  list.enabled
                    ? "border-white/10 bg-white/[0.02]"
                    : "border-white/5 bg-white/[0.01] opacity-60"
                }`}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-xs font-medium rounded px-1.5 py-0.5 ${
                        list.kind === "allow"
                          ? "bg-emerald-500/10 text-emerald-400"
                          : "bg-red-500/10 text-red-400"
                      }`}
                    >
                      {list.kind}
                    </span>
                    <span className="font-medium text-sm text-slate-200 truncate">
                      {list.name || list.url}
                    </span>
                  </div>
                  <p className="mt-0.5 text-xs text-slate-500 truncate">{list.url}</p>
                  <p className="mt-0.5 text-xs text-slate-500">
                    {list.domain_count} domains
                    {list.last_status ? ` · ${list.last_status}` : ""}
                  </p>
                </div>
                <div className="flex items-center gap-2 ml-4">
                  <button
                    onClick={() => handleToggle(list.id, list.enabled)}
                    className="text-xs text-slate-400 hover:text-slate-200"
                  >
                    {list.enabled ? "Disable" : "Enable"}
                  </button>
                  <button
                    onClick={() => handleDelete(list.id)}
                    className="text-xs text-red-400 hover:text-red-300"
                  >
                    Delete
                  </button>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <p className="text-sm text-slate-400">No adlists configured.</p>
        )}
      </div>
    </div>
  );
}
