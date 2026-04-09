import { fetchLogs } from "@/lib/api";
import { QueryLogClient } from "./client";
import { LiveTail } from "./live-tail";

export const dynamic = "force-dynamic";

export default async function QueryLogPage() {
  const logsResult = await fetchLogs(100).catch(() => null);

  return (
    <main className="mx-auto w-full max-w-7xl px-6 py-8 md:py-12">
      <h1 className="mb-6 text-2xl font-semibold text-slate-100">Query Log</h1>
      <QueryLogClient initial={logsResult} />
      <LiveTail />
    </main>
  );
}
