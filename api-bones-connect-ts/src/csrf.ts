// SPDX-License-Identifier: MIT
// The BFF CSRF synchronizer's replay rule, shared by every caller so none
// restates it. The Rust crate exports the same rule as `csrf_replay_token`.

/** The request and response header that carries the synchronizer token. */
export const CSRF_HEADER = "X-CSRF-Token";

/**
 * The token to replay a refused call with, or null when it must not be
 * replayed: exactly when the refusal is a 403 carrying a token different from
 * the one sent (including when none was sent).
 */
export function csrfReplayToken(
  status: number,
  refusalToken: string | null,
  sentToken: string | null,
): string | null {
  if (status !== 403 || !refusalToken || refusalToken === sentToken) return null;
  return refusalToken;
}
