"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const nav = [
  { href: "/", label: "Dashboard", icon: "◉" },
  { href: "/query-log", label: "Query Log", icon: "☰" },
  { href: "/lists", label: "Adlists", icon: "◈" },
  { href: "/domains", label: "Domains", icon: "⊘" },
  { href: "/heuristics", label: "Heuristics", icon: "⚡" },
  { href: "/groups", label: "Groups", icon: "⊞" },
  { href: "/clients", label: "Clients", icon: "⊡" },
  { href: "/settings", label: "Settings", icon: "⚙" },
];

export function Sidebar() {
  const pathname = usePathname();

  return (
    <aside className="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-white/10 bg-black/40 backdrop-blur-xl">
      <div className="flex items-center gap-2 px-5 py-6">
        <div className="h-2.5 w-2.5 rounded-full bg-signal shadow-[0_0_8px_rgba(82,255,168,0.6)]" />
        <span className="text-xs font-bold uppercase tracking-[0.2em] text-slate-300">
          Sentinel DNS
        </span>
      </div>

      <nav className="flex-1 space-y-0.5 px-3">
        {nav.map((item) => {
          const active = item.href === "/" ? pathname === "/" : pathname.startsWith(item.href);
          return (
            <Link
              key={item.href}
              href={item.href}
              className={`flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors ${
                active
                  ? "bg-white/10 text-white"
                  : "text-slate-400 hover:bg-white/5 hover:text-slate-200"
              }`}
            >
              <span className="w-4 text-center opacity-60">{item.icon}</span>
              {item.label}
            </Link>
          );
        })}
      </nav>

      <div className="space-y-2 px-5 py-4 text-[10px] text-slate-600">
        <a
          href="https://github.com/your-username/sentinel-dns"
          target="_blank"
          rel="noreferrer"
          className="block rounded border border-white/10 bg-white/5 px-2 py-1 text-center text-[10px] text-cyan-electric/90 transition-colors hover:bg-white/10 hover:text-cyan-electric"
        >
          Star Sentinel on GitHub
        </a>
        <div>Sentinel DNS v0.1.0</div>
      </div>
    </aside>
  );
}
