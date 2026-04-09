"use client";

import { useCallback, useState } from "react";

interface Props {
  onDismiss: () => void;
}

export function SetupWizard({ onDismiss }: Props) {
  const [step, setStep] = useState(0);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<"idle" | "pass" | "fail">(
    "idle"
  );

  const testConnection = useCallback(async () => {
    setTesting(true);
    setTestResult("idle");
    try {
      const res = await fetch("/api/proxy/resolve", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          client_id: "setup-wizard",
          query_domain: "example.com",
        }),
      });
      setTestResult(res.ok ? "pass" : "fail");
    } catch {
      setTestResult("fail");
    } finally {
      setTesting(false);
    }
  }, []);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 bg-black/70 backdrop-blur-sm"
        onClick={onDismiss}
      />
      <div className="relative w-full max-w-md animate-fade-in rounded-2xl border border-white/[0.12] bg-ink/95 p-6 shadow-2xl backdrop-blur-xl">
        {/* Close */}
        <button
          onClick={onDismiss}
          className="absolute right-4 top-4 text-slate-500 transition-colors hover:text-slate-300"
          aria-label="Close"
        >
          ✕
        </button>

        {/* Title */}
        <div className="mb-5 flex items-center gap-3">
          <div className="flex h-8 w-8 items-center justify-center rounded-full bg-signal/10">
            <span className="h-3 w-3 rounded-full bg-signal shadow-[0_0_12px_rgba(82,255,168,0.5)]" />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-white">
              Welcome to Sentinel DNS
            </h2>
            <p className="text-[10px] text-slate-500">
              Let&apos;s get your network protected
            </p>
          </div>
        </div>

        {/* Progress */}
        <div className="mb-5 flex gap-1">
          {[0, 1, 2].map((s) => (
            <div
              key={s}
              className={`h-0.5 flex-1 rounded-full transition-colors ${
                s <= step ? "bg-signal" : "bg-white/10"
              }`}
            />
          ))}
        </div>

        {step === 0 && (
          <div className="space-y-4">
            <h3 className="text-sm font-medium text-slate-200">
              Step 1: Find your Sentinel IP
            </h3>
            <div className="rounded-lg bg-white/[0.04] p-3">
              <p className="text-xs text-slate-400">
                Your Sentinel IP address is:
              </p>
              <p className="mt-1 font-mono text-lg text-signal">
                {typeof window !== "undefined"
                  ? window.location.hostname
                  : "loading..."}
              </p>
            </div>
            <p className="text-xs leading-relaxed text-slate-400">
              This is the IP address you&apos;ll set as the DNS server on
              your router. Write it down or keep this tab open.
            </p>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-4">
            <h3 className="text-sm font-medium text-slate-200">
              Step 2: Update your router DNS
            </h3>
            <p className="text-xs leading-relaxed text-slate-400">
              Log into your router admin panel (usually{" "}
              <code className="rounded bg-white/5 px-1 text-slate-300">
                192.168.1.1
              </code>{" "}
              or{" "}
              <code className="rounded bg-white/5 px-1 text-slate-300">
                192.168.0.1
              </code>
              ) and find the DHCP/DNS settings.
            </p>
            <div className="space-y-2">
              <RouterGuide
                brand="Most Routers"
                path="LAN Settings → DHCP → DNS Server"
              />
              <RouterGuide
                brand="TP-Link"
                path="DHCP → DHCP Settings → Primary DNS"
              />
              <RouterGuide
                brand="ASUS"
                path="LAN → DHCP Server → DNS Server"
              />
              <RouterGuide
                brand="Netgear"
                path="Internet → DNS Addresses → Use These"
              />
              <RouterGuide
                brand="UniFi"
                path="Networks → Default → DHCP DNS Server"
              />
            </div>
            <p className="text-xs text-slate-500">
              Set the Primary DNS to your Sentinel IP. Leave Secondary blank
              or set it to the same IP.
            </p>
          </div>
        )}

        {step === 2 && (
          <div className="space-y-4">
            <h3 className="text-sm font-medium text-slate-200">
              Step 3: Test your connection
            </h3>
            <p className="text-xs leading-relaxed text-slate-400">
              After updating your router, click the button below to verify
              Sentinel is resolving DNS queries correctly.
            </p>
            <button
              onClick={testConnection}
              disabled={testing}
              className="w-full rounded-lg bg-signal/20 py-2.5 text-sm font-medium text-signal transition-colors hover:bg-signal/30 disabled:opacity-50"
            >
              {testing ? "Testing..." : "Test Connection"}
            </button>
            {testResult === "pass" && (
              <div className="rounded-lg border border-signal/20 bg-signal/5 px-3 py-2 text-xs text-signal">
                DNS resolution is working. You&apos;re protected.
              </div>
            )}
            {testResult === "fail" && (
              <div className="rounded-lg border border-red-500/20 bg-red-500/5 px-3 py-2 text-xs text-red-400">
                Connection test failed. Make sure the backend is running and
                your device is using Sentinel as its DNS server.
              </div>
            )}
          </div>
        )}

        {/* Nav */}
        <div className="mt-6 flex justify-between">
          <button
            onClick={() => setStep((s) => Math.max(0, s - 1))}
            className={`rounded-lg px-4 py-2 text-xs text-slate-400 transition-colors hover:text-white ${
              step === 0 ? "invisible" : ""
            }`}
          >
            Back
          </button>
          {step < 2 ? (
            <button
              onClick={() => setStep((s) => s + 1)}
              className="rounded-lg bg-white/[0.08] px-4 py-2 text-xs font-medium text-white transition-colors hover:bg-white/[0.12]"
            >
              Next
            </button>
          ) : (
            <button
              onClick={onDismiss}
              className="rounded-lg bg-signal/20 px-4 py-2 text-xs font-medium text-signal transition-colors hover:bg-signal/30"
            >
              Done
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function RouterGuide({ brand, path }: { brand: string; path: string }) {
  return (
    <div className="flex items-center justify-between rounded-lg bg-white/[0.03] px-3 py-2">
      <span className="text-xs font-medium text-slate-300">{brand}</span>
      <span className="font-mono text-[10px] text-slate-500">{path}</span>
    </div>
  );
}
