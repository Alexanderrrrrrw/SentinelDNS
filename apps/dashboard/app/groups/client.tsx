"use client";

import { useState } from "react";
import type { Group } from "@/lib/api";

export function GroupsClient({ initial }: { initial: Group[] }) {
  const [groups, setGroups] = useState(initial);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");

  async function handleAdd() {
    if (!name.trim()) return;
    const res = await fetch("/api/proxy/groups", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, description: description || undefined }),
    });
    if (res.ok) {
      const group = await res.json();
      setGroups((prev) => [...prev, group]);
      setName("");
      setDescription("");
    }
  }

  async function handleDelete(id: number) {
    if (id === 0) return;
    await fetch(`/api/proxy/groups/${id}`, { method: "DELETE" });
    setGroups((prev) => prev.filter((g) => g.id !== id));
  }

  return (
    <div className="space-y-6">
      <div className="panel p-5">
        <h2 className="mb-3 text-lg font-semibold">Add Group</h2>
        <div className="flex flex-wrap gap-3">
          <input
            className="flex-1 min-w-[200px] rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="Group name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <input
            className="flex-1 min-w-[200px] rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-sm text-slate-100 placeholder-slate-500 focus:border-signal/50 focus:outline-none"
            placeholder="Description (optional)"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
          <button
            onClick={handleAdd}
            className="rounded-lg bg-signal/20 px-4 py-2 text-sm font-medium text-signal hover:bg-signal/30 transition-colors"
          >
            Add Group
          </button>
        </div>
      </div>

      <div className="panel p-5">
        <h2 className="mb-4 text-lg font-semibold">Groups ({groups.length})</h2>
        <div className="space-y-3">
          {groups.map((group) => (
            <div
              key={group.id}
              className="flex items-center justify-between rounded-lg border border-white/10 bg-white/[0.02] px-4 py-3"
            >
              <div>
                <p className="text-sm font-medium text-slate-200">
                  {group.name}
                  {group.id === 0 && (
                    <span className="ml-2 text-xs text-slate-500">(default)</span>
                  )}
                </p>
                {group.description && (
                  <p className="mt-0.5 text-xs text-slate-500">{group.description}</p>
                )}
              </div>
              {group.id !== 0 && (
                <button
                  onClick={() => handleDelete(group.id)}
                  className="text-xs text-red-400 hover:text-red-300"
                >
                  Delete
                </button>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
