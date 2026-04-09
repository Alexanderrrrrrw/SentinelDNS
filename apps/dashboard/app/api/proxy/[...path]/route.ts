import { NextRequest, NextResponse } from "next/server";

function getBaseUrl(): string {
  return process.env.SENTINEL_API_URL ?? "http://127.0.0.1:8080";
}

function getAdminToken(): string {
  return process.env.SENTINEL_ADMIN_TOKEN ?? "";
}

async function proxyRequest(
  req: NextRequest,
  params: Promise<{ path: string[] }>
): Promise<NextResponse> {
  const { path } = await params;
  const apiPath = path.join("/");
  const url = `${getBaseUrl()}/api/${apiPath}`;

  const headers: Record<string, string> = {};
  const token = getAdminToken();
  if (token) {
    headers["x-admin-token"] = token;
  }

  const contentType = req.headers.get("content-type");
  if (contentType) {
    headers["content-type"] = contentType;
  }

  try {
    const fetchOpts: RequestInit = {
      method: req.method,
      headers,
    };

    if (req.method !== "GET" && req.method !== "HEAD") {
      fetchOpts.body = await req.text();
    }

    const res = await fetch(url, fetchOpts);
    const body = await res.text();

    return new NextResponse(body, {
      status: res.status,
      headers: {
        "content-type": res.headers.get("content-type") ?? "application/json",
      },
    });
  } catch (e) {
    return NextResponse.json(
      { error: "Failed to reach backend", detail: String(e) },
      { status: 502 }
    );
  }
}

export async function GET(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return proxyRequest(req, context.params);
}

export async function POST(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return proxyRequest(req, context.params);
}

export async function PUT(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return proxyRequest(req, context.params);
}

export async function DELETE(
  req: NextRequest,
  context: { params: Promise<{ path: string[] }> }
) {
  return proxyRequest(req, context.params);
}
