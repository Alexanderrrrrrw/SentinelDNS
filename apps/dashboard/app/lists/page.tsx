import { fetchAdlists } from "@/lib/api";
import { ListsClient } from "./client";

export const dynamic = "force-dynamic";

export default async function ListsPage() {
  const lists = await fetchAdlists().catch(() => []);

  return (
    <main className="mx-auto w-full max-w-7xl px-6 py-8 md:py-12">
      <h1 className="mb-6 text-2xl font-semibold text-slate-100">Adlists</h1>
      <ListsClient initial={lists} />
    </main>
  );
}
