"use client";

import { useCallback, useEffect, useRef, useState } from "react";

interface LiveEntry {
  timestamp: string;
  client_id: string;
  client_name?: string | null;
  query_domain: string;
  action: string;
  protocol: string;
  response_time_ms: number;
}

const MAX_ENTRIES = 100;

export function LiveTail() {
  const [entries, setEntries] = useState<LiveEntry[]>([]);
  const [connected, setConnected] = useState(false);
  const [paused, setPaused] = useState(false);
  const [streaming, setStreaming] = useState(false);
  const [totalSeen, setTotalSeen] = useState(0);
  const eventSourceRef = useRef<EventSource | null>(null);
  const bufferRef = useRef<LiveEntry[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);

  const connect = useCallback(() => {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
    }

    const es = new EventSource("/api/proxy/logs/live");
    eventSourceRef.current = es;

    es.onopen = () => {
      setConnected(true);
      setStreaming(true);
    };

    es.onmessage = (event) => {
      try {
        const entry = JSON.parse(event.data) as LiveEntry;
        setTotalSeen((n) => n + 1);
        if (!paused) {
          setEntries((prev) => {
            const next = [entry, ...prev];
            if (next.length > MAX_ENTRIES) next.length = MAX_ENTRIES;
            return next;
          });
        } else {
          bufferRef.current.push(entry);
          if (bufferRef.current.length > MAX_ENTRIES) {
            bufferRef.current = bufferRef.current.slice(-MAX_ENTRIES);
          }
        }
      } catch {
        /* ignore malformed events */
      }
    };

    es.onerror = () => {
      setConnected(false);
      setStreaming(false);
    };
  }, [paused]);

  const disconnect = useCallback(() => {
    eventSourceRef.current?.close();
    eventSourceRef.current = null;
    setConnected(false);
    setStreaming(false);
  }, []);

  const togglePause = useCallback(() => {
    setPaused((p) => {
      if (p) {
        setEntries((prev) => {
          const merged = [...bufferRef.current.reverse(), ...prev];
          if (merged.length > MAX_ENTRIES) merged.length = MAX_ENTRIES;
          return merged;
        });
        bufferRef.current = [];
      }
      return !p;
    });
  }, []);

  const clear = useCallback(() => {
    setEntries([]);
    bufferRef.current = [];
    setTotalSeen(0);
  }, []);

  useEffect(() => {
    return () => {
      eventSourceRef.current?.close();
    };
  }, []);

  return (
    <div className="mt-8">
      {/* Header bar */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <h2 className="text-lg font-semibold text-slate-100">Live Tail</h2>
          <StatusDot connected={connected} paused={paused} />
          {streaming && (
            <span className="text-xs text-slate-500 tabular-nums">
              {totalSeen} queries seen
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {!streaming ? (
            <button
              onClick={connect}
              className="rounded-md bg-emerald-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-emerald-500"
            >
              Start
            </button>
          ) : (
            <>
              <button
                onClick={togglePause}
                className={`rounded-md px-3 py-1.5 text-xs font-medium transition-colors ${
                  paused
                    ? "bg-amber-600/20 text-amber-400 hover:bg-amber-600/30"
                    : "bg-slate-700 text-slate-300 hover:bg-slate-600"
                }`}
              >
                {paused ? "Resume" : "Pause"}
              </button>
              <button
                onClick={disconnect}
                className="rounded-md bg-red-600/20 px-3 py-1.5 text-xs font-medium text-red-400 transition-colors hover:bg-red-600/30"
              >
                Stop
              </button>
            </>
          )}
          <button
            onClick={clear}
            className="rounded-md bg-slate-700/50 px-3 py-1.5 text-xs font-medium text-slate-400 transition-colors hover:bg-slate-600/50"
          >
            Clear
          </button>
        </div>
      </div>

      {/* Stream output */}
      <div
        ref={scrollRef}
        className="panel overflow-hidden"
      >
        {entries.length === 0 ? (
          <div className="flex items-center justify-center py-12 text-sm text-slate-500">
            {streaming
              ? "Waiting for queries..."
              : 'Click "Start" to stream live DNS queries'}
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-white/10 text-xs uppercase tracking-wider text-slate-400">
                  <th className="px-4 py-2">Time</th>
                  <th className="px-4 py-2">Domain</th>
                  <th className="px-4 py-2">Client</th>
                  <th className="px-4 py-2">Action</th>
                  <th className="px-4 py-2">Protocol</th>
                  <th className="px-4 py-2">Latency</th>
                </tr>
              </thead>
              <tbody>
                {entries.map((entry, i) => (
                  <LiveRow key={`${entry.timestamp}-${i}`} entry={entry} isNew={i === 0} />
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}

function LiveRow({ entry, isNew }: { entry: LiveEntry; isNew: boolean }) {
  const isBlocked = entry.action === "blocked";
  return (
    <tr
      className={`border-b border-white/5 transition-colors duration-700 ${
        isNew && isBlocked
          ? "animate-shake"
          : isNew
            ? "animate-flash"
            : ""
      }`}
    >
      <td className="px-4 py-1.5 text-slate-400 tabular-nums text-xs">
        {formatTime(entry.timestamp)}
      </td>
      <td className="px-4 py-1.5 font-mono text-slate-200 text-xs">
        {entry.query_domain}
      </td>
      <td className="px-4 py-1.5 text-slate-400 text-xs">
        {entry.client_name ? (
          <span title={entry.client_id}>
            <span className="text-slate-300">{entry.client_name}</span>
            <span className="ml-1 text-slate-600 text-[10px]">{entry.client_id}</span>
          </span>
        ) : (
          entry.client_id
        )}
      </td>
      <td className="px-4 py-1.5">
        <ActionBadge action={entry.action} />
      </td>
      <td className="px-4 py-1.5 text-slate-400 text-xs uppercase">
        {entry.protocol}
      </td>
      <td className="px-4 py-1.5 tabular-nums text-slate-400 text-xs">
        {entry.response_time_ms}ms
      </td>
    </tr>
  );
}

function StatusDot({ connected, paused }: { connected: boolean; paused: boolean }) {
  if (!connected) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-slate-500">
        <span className="h-2 w-2 rounded-full bg-slate-600" />
        Disconnected
      </span>
    );
  }
  if (paused) {
    return (
      <span className="flex items-center gap-1.5 text-xs text-amber-400">
        <span className="h-2 w-2 rounded-full bg-amber-500" />
        Paused
      </span>
    );
  }
  return (
    <span className="flex items-center gap-1.5 text-xs text-emerald-400">
      <span className="h-2 w-2 animate-pulse rounded-full bg-emerald-500" />
      Streaming
    </span>
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
    <span
      className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${color}`}
    >
      {action}
    </span>
  );
}

function formatTime(ts: string): string {
  try {
    return new Date(ts).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      fractionalSecondDigits: 3,
    });
  } catch {
    return ts;
  }
}
