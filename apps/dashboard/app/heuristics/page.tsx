import { fetchHeuristicsStatus } from "@/lib/api";
import { HeuristicsClient } from "./client";

export const dynamic = "force-dynamic";

export default async function HeuristicsPage() {
  const status = await fetchHeuristicsStatus();

  return (
    <main className="mx-auto w-full max-w-5xl px-6 py-8 md:py-12">
      <header className="mb-8">
        <h1 className="text-2xl font-semibold text-slate-100">
          Heuristic Domain Analysis
        </h1>
        <p className="mt-2 text-sm text-slate-400">
          Sentinel&apos;s AI-free heuristic engine detects suspicious domains no
          blocklist has ever seen. It analyzes domain structure for DGA patterns,
          tracking infrastructure, and abuse signals.
        </p>
      </header>
      <HeuristicsClient initialStatus={status} />
    </main>
  );
}
