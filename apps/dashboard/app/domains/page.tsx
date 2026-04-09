import { fetchDomainRules } from "@/lib/api";
import { DomainsClient } from "./client";

export const dynamic = "force-dynamic";

export default async function DomainsPage() {
  const rules = await fetchDomainRules().catch(() => []);

  return (
    <main className="mx-auto w-full max-w-7xl px-6 py-8 md:py-12">
      <h1 className="mb-6 text-2xl font-semibold text-slate-100">Domain Rules</h1>
      <DomainsClient initial={rules} />
    </main>
  );
}
