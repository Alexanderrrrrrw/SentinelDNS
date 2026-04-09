import { NextRequest } from "next/server";

function getBaseUrl(): string {
  return process.env.SENTINEL_API_URL ?? "http://127.0.0.1:8080";
}

function getAdminToken(): string {
  return process.env.SENTINEL_ADMIN_TOKEN ?? "";
}

export async function GET(req: NextRequest) {
  const url = `${getBaseUrl()}/api/logs/live`;
  const headers: Record<string, string> = { Accept: "text/event-stream" };
  const token = getAdminToken();
  if (token) {
    headers["x-admin-token"] = token;
  }

  const upstream = await fetch(url, {
    headers,
    signal: req.signal,
  });

  if (!upstream.ok || !upstream.body) {
    return new Response(JSON.stringify({ error: "Failed to connect to live tail" }), {
      status: upstream.status || 502,
      headers: { "content-type": "application/json" },
    });
  }

  return new Response(upstream.body, {
    status: 200,
    headers: {
      "content-type": "text/event-stream",
      "cache-control": "no-cache, no-transform",
      connection: "keep-alive",
    },
  });
}
