import { fetchGroups } from "@/lib/api";
import { GroupsClient } from "./client";

export const dynamic = "force-dynamic";

export default async function GroupsPage() {
  const groups = await fetchGroups().catch(() => []);

  return (
    <main className="mx-auto w-full max-w-7xl px-6 py-8 md:py-12">
      <h1 className="mb-6 text-2xl font-semibold text-slate-100">Groups</h1>
      <GroupsClient initial={groups} />
    </main>
  );
}
