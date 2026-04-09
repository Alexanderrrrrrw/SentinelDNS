"use client";

import { useState } from "react";
import type { Client, Group } from "@/lib/api";

export function ClientsClient({
  initial,
  groups,
}: {
  initial: Client[];
  groups: Group[];
}) {
  const [clients, setClients] = useState(initial);
  const [ip, setIp] = useState("");
  const [name, setName] = useState("");
  const [selectedGroups, setSelectedGroups] = useState<number[]>([]);

  async function handleAdd() {
    if (!ip.trim()) return;
    const res = await fetch("/api/proxy/clients", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        ip,
        name: name || undefined,
        group_ids: selectedGroups.length > 0 ? selectedGroups : [0],
      }),
    });
    if (res.ok) {
      const client = await res.json();
      setClients((prev) => {
        const existing = prev.findIndex((c) => c.ip === client.ip);
        if (existing >= 0) {
          const updated = [...prev];
          updated[existing] = client;
          return updated;
        }
        return [...prev, client];
      });
      setIp("");
      setName("");
      setSelectedGroups([]);
    }
  }

  async function handleDelete(id: number) {
    await fetch(`/api/proxy/clients/${id}`, { method: "DELETE" });
    setClients((prev) => prev.filter((c) => c.id !== id));
  }

  function toggleGroup(gid: number) {
    setSelectedGroups((prev) =>
      prev.includes(gid) ? prev.filter((g) => g !== gid) : [...prev, gid]
    );
  }

  return (
    <div className="space-y-6">
      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Add / Update Client</h2>
        <div className="flex flex-wrap gap-3 mb-3">
          <input
            className="w-48 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="192.168.1.100"
            value={ip}
            onChange={(e) => setIp(e.target.value)}
          />
          <input
            className="w-48 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="Device name (optional)"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <button
            onClick={handleAdd}
            className="rounded-lg bg-signal/20 px-4 py-2 text-sm font-medium text-signal hover:bg-signal/30 transition-colors"
          >
            Save Client
          </button>
        </div>
        <div className="flex flex-wrap gap-2">
          {groups.map((g) => (
            <button
              key={g.id}
              onClick={() => toggleGroup(g.id)}
              className={`rounded-full border px-3 py-1 text-xs transition-colors ${
                selectedGroups.includes(g.id)
                  ? "border-signal/50 bg-signal/10 text-signal"
                  : "border-white/10 text-slate-400 hover:border-white/20"
              }`}
            >
              {g.name}
            </button>
          ))}
        </div>
      </div>

      <div className="panel p-5">
        <h2 className="mb-4 text-lg font-semibold">Known Clients ({clients.length})</h2>
        {clients.length > 0 ? (
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                  <th className="pb-2 pr-4">IP</th>
                  <th className="pb-2 pr-4">Name</th>
                  <th className="pb-2 pr-4">Groups</th>
                  <th className="pb-2">Actions</th>
                </tr>
              </thead>
              <tbody>
                {clients.map((client) => (
                  <tr key={client.id} className="border-b border-white/5">
                    <td className="py-2 pr-4 font-mono text-slate-200">{client.ip}</td>
                    <td className="py-2 pr-4 text-slate-400">{client.name ?? "—"}</td>
                    <td className="py-2 pr-4">
                      <div className="flex flex-wrap gap-1">
                        {client.group_ids.map((gid) => {
                          const group = groups.find((g) => g.id === gid);
                          return (
                            <span
                              key={gid}
                              className="inline-block rounded-full bg-white/5 px-2 py-0.5 text-xs text-slate-300"
                            >
                              {group?.name ?? `#${gid}`}
                            </span>
                          );
                        })}
                      </div>
                    </td>
                    <td className="py-2">
                      <button
                        onClick={() => handleDelete(client.id)}
                        className="text-xs text-red-400 hover:text-red-300"
                      >
                        Delete
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          <p className="text-sm text-slate-400">No clients configured.</p>
        )}
      </div>
    </div>
  );
}
