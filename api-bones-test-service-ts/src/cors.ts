// SPDX-License-Identifier: MIT

/** The hosts a browser page served from this machine is loaded from. */
const LOOPBACK_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

/** How long a browser may keep an answer to a preflight, in seconds. */
const PREFLIGHT_MAX_AGE_SECONDS = 600;

/** The answer to a preflight: a status and the headers it carries. */
export interface Preflight {
  status: number;
  headers: Record<string, string>;
}

/** Whether `origin` is the bare origin (scheme, host, port) of a page served from loopback over http or https. */
export function isLoopbackOrigin(origin: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(origin);
  } catch {
    return false;
  }
  return (parsed.protocol === "http:" || parsed.protocol === "https:") && parsed.origin === origin && LOOPBACK_HOSTS.has(parsed.hostname);
}

/**
 * The answer to a CORS preflight, or `undefined` when the request is not one
 * (it is not an `OPTIONS` or names no origin) and belongs to the service.
 *
 * A loopback origin is allowed to call with the headers it asks for; any
 * other origin is refused with no CORS header at all.
 */
export function preflight(method: string | undefined, origin: string | undefined, requestHeaders: string | undefined): Preflight | undefined {
  if (method !== "OPTIONS" || origin === undefined) return undefined;
  if (!isLoopbackOrigin(origin)) return { status: 403, headers: { vary: "origin" } };
  return {
    status: 204,
    headers: {
      "access-control-allow-origin": origin,
      "access-control-allow-methods": "GET, POST, OPTIONS",
      "access-control-allow-headers": requestHeaders ?? "",
      "access-control-max-age": String(PREFLIGHT_MAX_AGE_SECONDS),
      vary: "origin, access-control-request-headers",
    },
  };
}
