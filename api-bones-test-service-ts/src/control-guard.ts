// SPDX-License-Identifier: MIT

/** Why the control server refuses a request, or `undefined` when it serves it. */
export interface Refusal {
  status: number;
  error: string;
}

/**
 * The control port answers only the steps process: the `Host` must be exactly
 * `127.0.0.1:<port>` (a rebound DNS name carries another host) and every POST
 * must declare `application/json` (a cross-site form or no-cors fetch cannot).
 */
export function refuse(
  request: { method?: string; headers: { host?: string; "content-type"?: string } },
  port: number,
): Refusal | undefined {
  if (request.headers.host !== `127.0.0.1:${port}`) {
    return { status: 403, error: "the control port answers Host 127.0.0.1 only" };
  }
  if (request.method === "POST") {
    const essence = (request.headers["content-type"] ?? "").split(";")[0]!.trim().toLowerCase();
    if (essence !== "application/json") {
      return { status: 415, error: "the control port takes application/json only" };
    }
  }
  return undefined;
}
