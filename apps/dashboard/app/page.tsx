import {
  fetchGravityStatus,
  fetchHealth,
  fetchLogs,
  fetchStats,
  resolveSampleDomain,
} from "@/lib/api";
import { DashboardClient } from "./dashboard-client";

export const dynamic = "force-dynamic";

export default async function Page() {
  const [healthResult, statsResult, gravityResult, resolutionResult, logsResult] =
    await Promise.allSettled([
      fetchHealth(),
      fetchStats(),
      fetchGravityStatus(),
      resolveSampleDomain("example.com"),
      fetchLogs(20),
    ]);

  const apiOnline =
    healthResult.status === "fulfilled" ? healthResult.value : false;
  const stats =
    statsResult.status === "fulfilled" ? statsResult.value : null;
  const latestResolution =
    resolutionResult.status === "fulfilled" ? resolutionResult.value : null;
  const recentLogs =
    logsResult.status === "fulfilled" ? logsResult.value : null;
  const gravityStatus =
    gravityResult.status === "fulfilled" ? gravityResult.value : null;

  return (
    <DashboardClient
      apiOnline={apiOnline}
      stats={stats}
      gravityStatus={gravityStatus}
      latestResolution={latestResolution}
      recentLogs={recentLogs}
    />
  );
}
