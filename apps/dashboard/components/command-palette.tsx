"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";

interface Command {
  id: string;
  label: string;
  section: string;
  icon: string;
  action: () => void | Promise<void>;
  keywords?: string;
}

export function CommandPalette() {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const [feedback, setFeedback] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const showFeedback = useCallback((msg: string) => {
    setFeedback(msg);
    setTimeout(() => setFeedback(null), 2000);
  }, []);

  const apiAction = useCallback(
    async (path: string, method: string, body?: object) => {
      try {
        const res = await fetch(`/api/proxy/${path}`, {
          method,
          headers: body ? { "Content-Type": "application/json" } : undefined,
          body: body ? JSON.stringify(body) : undefined,
        });
        return res.ok;
      } catch {
        return false;
      }
    },
    []
  );

  const commands = useMemo<Command[]>(() => {
    const nav: Command[] = [
      { id: "nav-dashboard", label: "Go to Dashboard", section: "Navigation", icon: "◉", action: () => router.push("/"), keywords: "home" },
      { id: "nav-logs", label: "Go to Query Log", section: "Navigation", icon: "☰", action: () => router.push("/query-log"), keywords: "logs queries" },
      { id: "nav-adlists", label: "Go to Adlists", section: "Navigation", icon: "◈", action: () => router.push("/lists"), keywords: "blocklists" },
      { id: "nav-domains", label: "Go to Domains", section: "Navigation", icon: "⊘", action: () => router.push("/domains"), keywords: "rules allowlist denylist" },
      { id: "nav-heuristics", label: "Go to Heuristics", section: "Navigation", icon: "⚡", action: () => router.push("/heuristics"), keywords: "dga scoring" },
      { id: "nav-groups", label: "Go to Groups", section: "Navigation", icon: "⊞", action: () => router.push("/groups") },
      { id: "nav-clients", label: "Go to Clients", section: "Navigation", icon: "⊡", action: () => router.push("/clients"), keywords: "devices" },
      { id: "nav-settings", label: "Go to Settings", section: "Navigation", icon: "⚙", action: () => router.push("/settings"), keywords: "config" },
    ];

    const actions: Command[] = [
      {
        id: "action-gravity",
        label: "Update Gravity (pull blocklists)",
        section: "Actions",
        icon: "↻",
        keywords: "refresh update gravity pull",
        action: async () => {
          showFeedback("Pulling blocklists...");
          const ok = await apiAction("gravity/update", "POST");
          showFeedback(ok ? "Gravity updated" : "Gravity update failed");
        },
      },
      {
        id: "action-heur-on",
        label: "Enable Heuristic Engine",
        section: "Actions",
        icon: "⚡",
        keywords: "heuristic enable",
        action: async () => {
          const ok = await apiAction("heuristics/toggle", "PUT", { enabled: true });
          showFeedback(ok ? "Heuristics enabled" : "Failed");
        },
      },
      {
        id: "action-heur-off",
        label: "Disable Heuristic Engine",
        section: "Actions",
        icon: "⚡",
        keywords: "heuristic disable",
        action: async () => {
          const ok = await apiAction("heuristics/toggle", "PUT", { enabled: false });
          showFeedback(ok ? "Heuristics disabled" : "Failed");
        },
      },
      {
        id: "action-export",
        label: "Export Configuration",
        section: "Actions",
        icon: "↗",
        keywords: "export backup config",
        action: async () => {
          try {
            const res = await fetch("/api/proxy/config/export");
            if (!res.ok) { showFeedback("Export failed"); return; }
            const data = await res.json();
            const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
            const url = URL.createObjectURL(blob);
            const a = document.createElement("a");
            a.href = url;
            a.download = `sentinel-backup-${new Date().toISOString().slice(0, 10)}.json`;
            a.click();
            URL.revokeObjectURL(url);
            showFeedback("Config exported");
          } catch {
            showFeedback("Export failed");
          }
        },
      },
    ];

    const domainActions: Command[] = [];
    const trimmed = query.trim().toLowerCase();

    const blockMatch = trimmed.match(/^block\s+(.+)/);
    const whitelistMatch = trimmed.match(/^(?:allow|whitelist)\s+(.+)/);

    if (blockMatch && blockMatch[1]) {
      const domain = blockMatch[1];
      domainActions.push({
        id: `block-${domain}`,
        label: `Block ${domain}`,
        section: "Domain Actions",
        icon: "🚫",
        action: async () => {
          const ok = await apiAction("domains", "POST", {
            kind: "exact_deny",
            value: domain,
            comment: "Blocked via command palette",
          });
          showFeedback(ok ? `${domain} blocked` : "Block failed");
        },
      });
    }

    if (whitelistMatch && whitelistMatch[1]) {
      const domain = whitelistMatch[1];
      domainActions.push({
        id: `allow-${domain}`,
        label: `Whitelist ${domain}`,
        section: "Domain Actions",
        icon: "✓",
        action: async () => {
          const ok = await apiAction("domains", "POST", {
            kind: "exact_allow",
            value: domain,
            comment: "Whitelisted via command palette",
          });
          showFeedback(ok ? `${domain} whitelisted` : "Whitelist failed");
        },
      });
    }

    return [...domainActions, ...nav, ...actions];
  }, [query, router, apiAction, showFeedback]);

  const filtered = useMemo(() => {
    if (!query.trim()) return commands;
    const q = query.toLowerCase();
    return commands.filter(
      (c) =>
        c.label.toLowerCase().includes(q) ||
        c.section.toLowerCase().includes(q) ||
        (c.keywords && c.keywords.includes(q))
    );
  }, [query, commands]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.ctrlKey || e.metaKey) && e.key === "k") {
        e.preventDefault();
        setOpen((o) => !o);
        setQuery("");
        setFeedback(null);
      }
      if (e.key === "Escape" && open) {
        setOpen(false);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open]);

  useEffect(() => {
    if (open) {
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  const runCommand = useCallback((cmd: Command) => {
    cmd.action();
    if (cmd.section === "Navigation") {
      setOpen(false);
    }
  }, []);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setActiveIndex((i) => Math.min(i + 1, Math.max(0, filtered.length - 1)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setActiveIndex((i) => Math.max(i - 1, 0));
      } else if (e.key === "Enter" && filtered[activeIndex]) {
        e.preventDefault();
        runCommand(filtered[activeIndex]);
      }
    },
    [filtered, activeIndex, runCommand]
  );

  useEffect(() => {
    if (listRef.current) {
      const activeEl = listRef.current.querySelector(`[data-index="${activeIndex}"]`);
      activeEl?.scrollIntoView({ block: "nearest" });
    }
  }, [activeIndex]);

  if (!open) return null;

  const sections = new Map<string, Command[]>();
  for (const cmd of filtered) {
    const group = sections.get(cmd.section) || [];
    group.push(cmd);
    sections.set(cmd.section, group);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh]">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60 backdrop-blur-sm"
        onClick={() => setOpen(false)}
      />

      {/* Palette */}
      <div className="relative w-full max-w-lg animate-fade-in rounded-2xl border border-white/[0.12] bg-ink/95 shadow-2xl backdrop-blur-xl">
        {/* Input */}
        <div className="flex items-center gap-3 border-b border-white/[0.08] px-4 py-3">
          <span className="text-slate-500">⌘</span>
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={handleKeyDown}
            placeholder="Type a command or search... (e.g. 'block ads.com')"
            className="flex-1 bg-transparent text-sm text-slate-100 placeholder-slate-500 outline-none"
          />
          <kbd className="rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-500">
            ESC
          </kbd>
        </div>

        {/* Feedback toast */}
        {feedback && (
          <div className="border-b border-white/[0.06] bg-signal/5 px-4 py-2 text-xs text-signal">
            {feedback}
          </div>
        )}

        {/* Results */}
        <div ref={listRef} className="max-h-80 overflow-y-auto py-2">
          {filtered.length === 0 ? (
            <p className="px-4 py-6 text-center text-xs text-slate-500">
              No commands found
            </p>
          ) : (
            Array.from(sections.entries()).map(([section, cmds]) => (
              <div key={section}>
                <p className="px-4 pb-1 pt-2 text-[10px] font-medium uppercase tracking-widest text-slate-600">
                  {section}
                </p>
                {cmds.map((cmd) => {
                  const globalIdx = filtered.indexOf(cmd);
                  return (
                    <button
                      key={cmd.id}
                      data-index={globalIdx}
                      onClick={() => runCommand(cmd)}
                      onMouseEnter={() => setActiveIndex(globalIdx)}
                      className={`flex w-full items-center gap-3 px-4 py-2 text-left text-sm transition-colors ${
                        globalIdx === activeIndex
                          ? "bg-white/[0.06] text-white"
                          : "text-slate-400 hover:text-slate-200"
                      }`}
                    >
                      <span className="w-4 text-center text-xs opacity-60">
                        {cmd.icon}
                      </span>
                      {cmd.label}
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </div>

        {/* Footer hint */}
        <div className="flex items-center justify-between border-t border-white/[0.06] px-4 py-2 text-[10px] text-slate-600">
          <span>↑↓ navigate</span>
          <span>↵ select</span>
          <span>esc close</span>
        </div>
      </div>
    </div>
  );
}
