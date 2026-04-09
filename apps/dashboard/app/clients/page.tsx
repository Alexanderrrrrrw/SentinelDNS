import { fetchClients, fetchGroups } from "@/lib/api";
import { ClientsClient } from "./client";

export const dynamic = "force-dynamic";

export default async function ClientsPage() {
  const [clients, groups] = await Promise.all([
    fetchClients().catch(() => []),
    fetchGroups().catch(() => []),
  ]);

  return (
    <main className="mx-auto w-full max-w-7xl px-6 py-8 md:py-12">
      <h1 className="mb-6 text-2xl font-semibold text-slate-100">Clients</h1>
      <ClientsClient initial={clients} groups={groups} />
    </main>
  );
}
