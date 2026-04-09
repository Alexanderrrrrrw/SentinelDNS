"use client";

import { useState } from "react";
import type {
  HeuristicsStatus,
  HeuristicScoreResponse,
} from "@/lib/api";

interface Props {
  initialStatus: HeuristicsStatus | null;
}

export function HeuristicsClient({ initialStatus }: Props) {
  const [enabled, setEnabled] = useState(initialStatus?.heuristics_enabled ?? true);
  const [toggling, setToggling] = useState(false);
  const [domain, setDomain] = useState("");
  const [scoring, setScoring] = useState(false);
  const [result, setResult] = useState<HeuristicScoreResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<HeuristicScoreResponse[]>([]);

  async function handleToggle() {
    setToggling(true);
    try {
      const res = await fetch("/api/proxy/heuristics/toggle", {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled: !enabled }),
      });
      if (res.ok) setEnabled(!enabled);
    } finally {
      setToggling(false);
    }
  }

  async function handleScore() {
    if (!domain.trim()) return;
    setScoring(true);
    setError(null);
    try {
      const res = await fetch("/api/proxy/heuristics/score", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ domain: domain.trim() }),
      });
      if (!res.ok) {
        setError(`API returned ${res.status}`);
        return;
      }
      const data = (await res.json()) as HeuristicScoreResponse;
      setResult(data);
      setHistory((prev) => [data, ...prev.slice(0, 19)]);
    } catch (e) {
      setError(String(e));
    } finally {
      setScoring(false);
    }
  }

  const blockThreshold = initialStatus?.block_threshold ?? 70;
  const warnThreshold = initialStatus?.warn_threshold ?? 45;

  return (
    <div className="space-y-6">
      {/* Toggle */}
      <section className="panel flex items-center justify-between p-5">
        <div>
          <h2 className="text-sm font-semibold text-slate-200">
            Heuristic Blocking Engine
          </h2>
          <p className="mt-1 text-xs text-slate-400">
            When enabled, domains scoring &ge; {blockThreshold} are automatically blocked
            even if they appear on no blocklist.
          </p>
        </div>
        <button
          onClick={handleToggle}
          disabled={toggling}
          className={`rounded-full px-4 py-1.5 text-xs font-medium transition-colors ${
            enabled
              ? "bg-emerald-500/20 text-emerald-300 hover:bg-emerald-500/30"
              : "bg-red-500/20 text-red-300 hover:bg-red-500/30"
          }`}
        >
          {toggling ? "..." : enabled ? "Enabled" : "Disabled"}
        </button>
      </section>

      {/* Scanner */}
      <section className="panel p-5">
        <h2 className="mb-4 text-sm font-semibold text-slate-200">
          Domain Scanner
        </h2>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            handleScore();
          }}
          className="flex gap-2"
        >
          <input
            type="text"
            value={domain}
            onChange={(e) => setDomain(e.target.value)}
            placeholder="Enter a domain to analyze..."
            className="flex-1 rounded-lg border border-white/10 bg-black/30 px-4 py-2 text-sm text-slate-100 placeholder-slate-500 outline-none focus:border-blue-500/50"
          />
          <button
            type="submit"
            disabled={scoring || !domain.trim()}
            className="rounded-lg bg-blue-600 px-6 py-2 text-sm font-medium text-white hover:bg-blue-500 disabled:opacity-40"
          >
            {scoring ? "Analyzing..." : "Analyze"}
          </button>
        </form>

        {error && (
          <p className="mt-3 text-sm text-red-400">{error}</p>
        )}

        {result && <ScoreCard result={result} blockThreshold={blockThreshold} warnThreshold={warnThreshold} />}
      </section>

      {/* Example domains */}
      <section className="panel p-5">
        <h2 className="mb-3 text-sm font-semibold text-slate-200">
          Try These Examples
        </h2>
        <div className="flex flex-wrap gap-2">
          {[
            "google.com",
            "xkqz9m2np5v8w1rj.tk",
            "a1b2c3d4e5f6a7b8.tracker.com",
            "pixel-track.ad-network.xyz",
            "a.b.c.d.e.f.tracking.com",
            "legitimate-website.org",
            "click3847.mailchimp.com",
          ].map((d) => (
            <button
              key={d}
              onClick={() => {
                setDomain(d);
                setResult(null);
              }}
              className="rounded-md border border-white/10 bg-white/5 px-3 py-1 font-mono text-xs text-slate-300 hover:bg-white/10 hover:text-white transition-colors"
            >
              {d}
            </button>
          ))}
        </div>
      </section>

      {/* History */}
      {history.length > 0 && (
        <section className="panel p-5">
          <h2 className="mb-4 text-sm font-semibold text-slate-200">
            Scan History
          </h2>
          <div className="space-y-2">
            {history.map((h, i) => (
              <div
                key={`${h.domain}-${i}`}
                className="flex items-center justify-between rounded-lg border border-white/5 bg-black/20 px-4 py-2"
              >
                <span className="font-mono text-sm text-slate-200 truncate mr-3">
                  {h.domain}
                </span>
                <div className="flex items-center gap-3 shrink-0">
                  <span className="text-xs tabular-nums text-slate-400">
                    {h.score.toFixed(0)}pts
                  </span>
                  <VerdictBadge verdict={h.verdict} />
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {/* How it works */}
      <section className="panel p-5">
        <h2 className="mb-4 text-sm font-semibold text-slate-200">
          How It Works
        </h2>
        <p className="mb-3 text-xs text-slate-400">
          Pi-hole relies exclusively on static blocklists — if a domain
          isn&apos;t on a list, it passes. Sentinel goes further with 9
          structural heuristics that run on every uncategorized domain:
        </p>
        <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {[
            { name: "Shannon Entropy", desc: "Measures randomness in labels. DGA domains cluster at 3.5-4.0 bits/char." },
            { name: "Vowel/Consonant Ratio", desc: "Natural language has predictable ratios; random strings deviate heavily." },
            { name: "Numeric Density", desc: "Ad/tracking subdomains pack hex or decimal IDs into labels." },
            { name: "Subdomain Depth", desc: "Deeply nested domains (>5 labels) are often tracking infrastructure." },
            { name: "Domain Length", desc: "Unusually long FQDNs (>60 chars) are suspicious." },
            { name: "Suspicious TLD", desc: ".xyz, .tk, .top, .buzz, etc. are disproportionately used for abuse." },
            { name: "Hex Hash Labels", desc: "Labels matching [0-9a-f]{16+} look like C2 or tracking hashes." },
            { name: "Numeric-Only Labels", desc: "Entirely numeric labels are often ephemeral infrastructure." },
            { name: "Hyphen Density", desc: "Labels with >30% hyphens are common in DGA and phishing domains." },
          ].map((h) => (
            <div
              key={h.name}
              className="rounded-lg border border-white/5 bg-black/20 px-3 py-2"
            >
              <p className="text-xs font-medium text-slate-200">{h.name}</p>
              <p className="mt-1 text-[10px] leading-relaxed text-slate-500">
                {h.desc}
              </p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

function ScoreCard({
  result,
  blockThreshold,
  warnThreshold,
}: {
  result: HeuristicScoreResponse;
  blockThreshold: number;
  warnThreshold: number;
}) {
  const pct = Math.min((result.score / blockThreshold) * 100, 100);
  const barColor =
    result.verdict === "suspicious"
      ? "bg-red-500"
      : result.verdict === "warn"
      ? "bg-amber-500"
      : "bg-emerald-500";

  return (
    <div className="mt-4 rounded-xl border border-white/10 bg-black/30 p-5">
      <div className="flex items-center justify-between">
        <div>
          <p className="font-mono text-lg text-slate-100">{result.domain}</p>
          <p className="mt-1 text-xs text-slate-400">
            Score: <span className="font-semibold text-slate-200">{result.score.toFixed(1)}</span>
            {" / "}{blockThreshold} (warn at {warnThreshold})
          </p>
        </div>
        <VerdictBadge verdict={result.verdict} large />
      </div>

      {/* Score bar */}
      <div className="mt-4 h-2 w-full overflow-hidden rounded-full bg-white/5">
        <div
          className={`h-full rounded-full transition-all duration-500 ${barColor}`}
          style={{ width: `${pct}%` }}
        />
      </div>

      {/* Threshold markers */}
      <div className="relative mt-1 h-4">
        <span
          className="absolute text-[9px] text-amber-400/60"
          style={{ left: `${(warnThreshold / blockThreshold) * 100}%`, transform: "translateX(-50%)" }}
        >
          warn
        </span>
        <span
          className="absolute text-[9px] text-red-400/60"
          style={{ left: "100%", transform: "translateX(-100%)" }}
        >
          block
        </span>
      </div>

      {/* Signals */}
      {result.signals.length > 0 && (
        <div className="mt-4 space-y-2">
          <p className="text-xs font-medium text-slate-300">
            Triggered Signals ({result.signals.length})
          </p>
          {result.signals.map((sig, i) => (
            <div
              key={`${sig.name}-${i}`}
              className="flex items-center justify-between rounded-lg border border-white/5 bg-black/20 px-3 py-2"
            >
              <div>
                <span className="text-xs font-medium text-slate-200">
                  {sig.name.replace(/_/g, " ")}
                </span>
                <p className="text-[10px] text-slate-500">{sig.detail}</p>
              </div>
              <span className="font-mono text-xs font-semibold text-amber-300">
                +{sig.weight.toFixed(1)}
              </span>
            </div>
          ))}
        </div>
      )}

      {result.signals.length === 0 && (
        <p className="mt-4 text-xs text-slate-500">
          No suspicious signals detected. This domain appears structurally
          clean.
        </p>
      )}
    </div>
  );
}

function VerdictBadge({
  verdict,
  large,
}: {
  verdict: string;
  large?: boolean;
}) {
  const styles = {
    clean: "bg-emerald-500/15 text-emerald-400 border-emerald-500/20",
    warn: "bg-amber-500/15 text-amber-400 border-amber-500/20",
    suspicious: "bg-red-500/15 text-red-400 border-red-500/20",
  }[verdict] ?? "bg-slate-500/15 text-slate-400 border-slate-500/20";

  return (
    <span
      className={`inline-flex items-center rounded-full border font-medium ${styles} ${
        large ? "px-3 py-1 text-sm" : "px-2 py-0.5 text-[10px]"
      }`}
    >
      {verdict}
    </span>
  );
}
