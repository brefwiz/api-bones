// SPDX-License-Identifier: MIT
import assert from "node:assert/strict";
import { randomUUID, sign, verify } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { setTimeout as delay } from "node:timers/promises";
import { create, toBinary, type DescMethodUnary } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import { connectNodeAdapter } from "@connectrpc/connect-node";
import pg from "pg";
import { authorizeBatchResults, serveBatchRequest, encodeErrorInfo, errorInfoOf, type ItemResult } from "@brefwiz/api-bones-connect";
import type { BatchState } from "../steps/batch.ts";
export interface WidgetDatabase { databaseName?: string; databaseRole?: string; database?: pg.Client; readLock?: pg.Client; databaseTurn?: Promise<unknown>; listenerPort?: number }
const postgres = process.env.SKIP_TESTCONTAINERS === "1" ? undefined
  : await new (await import("@testcontainers/postgresql")).PostgreSqlContainer("postgres:17-alpine").start();
const adminUrl = postgres?.getConnectionUri() ?? process.env.DATABASE_URL;
assert.ok(adminUrl, "DATABASE_URL is required when SKIP_TESTCONTAINERS=1");
const postgresAdminUrl: string = adminUrl;
export const stopWidgetPostgres = async () => { await postgres?.stop(); };
const codes: Record<string, Code> = { invalid_argument: Code.InvalidArgument, not_found: Code.NotFound, permission_denied: Code.PermissionDenied };
function identityFromCredential(s: BatchState, authorization: string): { org: string; subject: string; permissions: readonly string[] } {
  const [header, body, signature] = authorization.replace(/^Bearer /, "").split(".");
  if (!header || !body || !signature || !verify(null, Buffer.from(`${header}.${body}`), s.keyPair.publicKey, Buffer.from(signature, "base64url"))) throw new ConnectError("invalid credential", Code.Unauthenticated);
  const claims = JSON.parse(Buffer.from(body, "base64url").toString());
  if (!claims.sub || !claims.org || claims.aud !== "contract.v1.Widgets" || claims.exp <= Date.now() / 1000) throw new ConnectError("invalid claims", Code.Unauthenticated);
  if (!Array.isArray(claims.permissions) || !claims.permissions.every((permission: unknown) => typeof permission === "string")) throw new ConnectError("invalid permissions", Code.Unauthenticated);
  return { org: claims.org, subject: claims.sub, permissions: claims.permissions };
}
interface WidgetRow { id: string; parts: string[]; owner: string | null; revision: string; }
async function authorizeWidget(row: WidgetRow, principal: string): Promise<void> {
  if (row.owner !== null && row.owner !== principal) throw new ConnectError("widget owner required", Code.PermissionDenied, undefined, [{ type: "bones.v1.ErrorInfo", value: encodeErrorInfo({ code: "WIDGET_OWNER_REQUIRED", emitter: "widgets" }) }]);
}
function itemRefusal(id: string, found: boolean) {
  if (found && !id.includes(" ")) return undefined;
  const malformed = id.includes(" ");
  return { code: malformed ? "invalid_argument" : "not_found", message: malformed ? "widget id is malformed" : "widget not found", info: { code: malformed ? "WIDGET_ID_MALFORMED" : "WIDGET_NOT_FOUND", emitter: "widgets" }, violations: malformed ? [{ field: "id", message: "widget id is malformed", rule: "format" }] : [] };
}
function refusal(error: NonNullable<ReturnType<typeof itemRefusal>>) {
  return new ConnectError(error.message, codes[error.code], undefined, [{ type: "bones.v1.ErrorInfo", value: encodeErrorInfo(error.info) }]);
}
async function inOrganization<T>(s: BatchState, org: string, operation: (db: pg.Client) => Promise<T>): Promise<T> {
  const db = s.database!;
  await db.query("BEGIN");
  try {
    await db.query("SELECT set_config('app.org', $1, true)", [org]);
    const answer = await operation(db);
    await db.query("COMMIT");
    return answer;
  } catch (error) { await db.query("ROLLBACK"); throw error; }
}
// pg.Client serializes queries, but transactions from different handlers must remain indivisible.
function serialized<T>(s: BatchState, operation: () => Promise<T>): Promise<T> {
  const answer = (s.databaseTurn ?? Promise.resolve()).then(operation);
  s.databaseTurn = answer.catch(() => undefined);
  return answer;
}
function scopedLookup(s: BatchState, org: string, ids: string[]): Promise<Map<string, WidgetRow>> {
  return serialized(s, () => inOrganization(s, org, async db => {
    const answer = await db.query<WidgetRow>("SELECT id, parts, owner, xmin::text AS revision FROM widgets WHERE id = ANY($1)", [ids]);
    return new Map(answer.rows.map(row => [row.id, row]));
  }));
}
function replaceParts(s: BatchState, org: string, id: string, parts: readonly string[], principal: string, owner?: string): Promise<WidgetRow | undefined> {
  return serialized(s, () => inOrganization(s, org, async db => {
    const locked = await db.query<WidgetRow>("SELECT id, parts, owner, xmin::text AS revision FROM widgets WHERE id=$1 FOR UPDATE", [id]);
    const row = locked.rows[0], error = itemRefusal(id, !!row);
    if (error) throw refusal(error);
    await authorizeWidget(row!, principal);
    const saved = await db.query<WidgetRow>("UPDATE widgets SET parts=$2, owner=COALESCE($3, owner) WHERE id=$1 RETURNING id, parts, owner, xmin::text AS revision", [id, parts, owner]);
    return saved.rows[0];
  }));
}
async function database(s: BatchState) {
  const suffix = randomUUID().replaceAll("-", "");
  s.databaseName = `batch_${suffix}`;
  s.databaseRole = `reader_${suffix}`;
  const owner = new pg.Client({ connectionString: postgresAdminUrl });
  await owner.connect();
  try { await owner.query(`CREATE DATABASE ${s.databaseName}`); }
  finally { await owner.end(); }
  const uri = new URL(postgresAdminUrl); uri.pathname = `/${s.databaseName}`;
  const admin = new pg.Client({ connectionString: uri.toString() });
  await admin.connect();
  try {
    await admin.query("CREATE TABLE widgets (org_id text NOT NULL, id text NOT NULL, parts text[] NOT NULL, owner text)");
    await admin.query("ALTER TABLE widgets ENABLE ROW LEVEL SECURITY");
    await admin.query("ALTER TABLE widgets FORCE ROW LEVEL SECURITY");
    await admin.query("CREATE POLICY organization ON widgets USING (org_id = current_setting('app.org', true))");
    const password = Buffer.from(sign(null, Buffer.from("database role"), s.keyPair.privateKey)).toString("hex");
    await admin.query(`CREATE ROLE ${s.databaseRole} LOGIN PASSWORD '${password}' NOSUPERUSER NOBYPASSRLS`);
    await admin.query(`GRANT SELECT, UPDATE ON widgets TO ${s.databaseRole}`);
    await admin.query("INSERT INTO widgets SELECT org,id,parts,owner FROM jsonb_to_recordset($1::jsonb) AS fixture(org text,id text,parts text[],owner text)", [JSON.stringify(s.fixtures)]);
    uri.username = s.databaseRole; uri.password = password;
    s.database = new pg.Client({ connectionString: uri.toString() });
    await s.database.connect();
    const role = await s.database.query("SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user");
    assert.deepEqual(role.rows, [{ rolsuper: false, rolbypassrls: false }]);
  } finally { await admin.end(); }
}
export async function startWidgetsProvider(s: BatchState) {
  await database(s);
  const handler = connectNodeAdapter({ readMaxBytes: 2 * 1048576, routes(router) {
    for (const method of s.service!.methods) router.rpc(method as DescMethodUnary, async (request, context) => {
      const input = request as unknown as { id?: string; ids?: string[]; widgetId?: string; widgetIds?: string[]; kind?: string; items?: string[]; owner?: string; page?: { cursor: string; pageSize: number } };
      const ids = input.ids ?? input.widgetIds ?? [input.id ?? input.widgetId!];
      const authorization = context.requestHeader.get("authorization") ?? "";
      const { org, subject, permissions } = identityFromCredential(s, authorization);
      if (!permissions.includes("widgets:read")) throw new ConnectError("widget read authority required", Code.PermissionDenied);
      if (method.name === "UpdateWidgetParts" && !permissions.includes("widgets:write")) throw new ConnectError("widget write authority required", Code.PermissionDenied, undefined, [{ type: "bones.v1.ErrorInfo", value: encodeErrorInfo({ code: "WIDGET_WRITE_REQUIRED", emitter: "widgets" }) }]);
      const cursor = input.page?.cursor ?? "";
      s.requests.push({ rpc: method.name, ids, authorization, cursor, pageSize: input.page?.pageSize || undefined, bytes: toBinary(method.input, request).length, trace: context.requestHeader.get("x-trace") ?? "", timeout: context.requestHeader.get("connect-timeout-ms"), cache: context.requestHeader.get("bones-list-cache"), revision: context.requestHeader.get("if-none-match"), lane: context.requestHeader.get("x-token-lane") ?? "" });
      const batch = method.name.startsWith("Batch");
      const aborted = () => s.events.emit("aborted");
      context.signal.addEventListener("abort", aborted, { once: true });
      if (batch) { s.active++; s.peak = Math.max(s.peak, s.active); }
      s.events.emit("request");
      try { return await serveBatchRequest(batch ? ids : [], async () => {
        if (s.release) await new Promise<void>(resolve => s.events.once("release", resolve));
        else if (s.delay) await new Promise(resolve => setTimeout(resolve, s.delay));
        if (batch) {
          if (s.failIdentityOnRequest) s.identityFailure = true;
          if (s.sourceFaultOnRequest) s.sourceFault = s.sourceFaultOnRequest;
          if (s.swapOnRequest) { s.session = s.swapOnRequest; s.swapOnRequest = undefined; }
          const rule = s.failures.find(rule => rule.remaining > 0 && (!rule.carrying || ids.includes(rule.carrying)));
          if (rule) { rule.remaining--; throw new ConnectError("scripted failure", rule.code); }
        }
        if (method.name === "UpdateWidgetParts") {
          const saved = await replaceParts(s, org, input.widgetId!, input.items ?? [], subject, input.owner);
          if (!saved) throw refusal(itemRefusal(input.widgetId!, false)!);
          return create(method.output, { items: saved.parts, page: { revision: saved.revision } } as never);
        }
        const rows = await scopedLookup(s, org, ids);
        if (method.name.endsWith("GetWidgets") || method.name === "GetWidget") {
          const outcomes: ItemResult<WidgetRow>[] = ids.map(id => {
            const row = rows.get(id);
            const error = itemRefusal(id, !!row);
            return error ? { ok: false, error: refusal(error) } : { ok: true, value: row! };
          });
          if (method.name === "GetWidget") {
            const outcome = outcomes[0];
            if (!outcome.ok) throw outcome.error;
            await authorizeWidget(outcome.value, subject);
            return create(method.output, { id: outcome.value.id, name: outcome.value.id, kind: input.kind ?? "" } as never);
          }
          const authorized = await authorizeBatchResults(outcomes, row => {
            if (s.authorizationFault?.id === row.id) throw new ConnectError(s.authorizationFault.message, Code.Internal);
            return authorizeWidget(row, subject);
          });
          const results = authorized.map((outcome, index) => ({ id: ids[index], outcome: outcome.ok ? { case: "item", value: { id: outcome.value.id, name: outcome.value.id, kind: input.kind ?? "" } } : { case: "error", value: { code: Object.keys(codes).find(code => codes[code] === outcome.error.code) ?? "unknown", message: outcome.error.rawMessage, info: errorInfoOf(outcome.error), violations: itemRefusal(ids[index], rows.has(ids[index]))?.violations ?? [] } } }));
          if (s.reverse || s.malformed === "order") results.reverse();
          if (s.malformed === "count") results.pop();
          return create(method.output, { results } as never);
        }
        const groups = [];
        for (const id of ids) {
          const row = rows.get(id);
          const error = itemRefusal(id, !!row);
          const start = Number(cursor.replace(/^c/, "")) || 0;
          const end = Math.min(start + (input.page?.pageSize || s.pageSize), row?.parts.length ?? 0);
          const page = { nextCursor: end < (row?.parts.length ?? 0) ? `c${end}` : "", revision: row?.revision ?? "" };
          if (method.name.startsWith("ListWidgetParts")) {
            if (error) throw refusal(error);
            await authorizeWidget(row!, subject);
            context.responseHeader.set("etag", `"${row!.revision}"`);
            if (!cursor && context.requestHeader.get("bones-list-cache") === "1" && context.requestHeader.get("if-none-match") === `"${row!.revision}"`) {
              context.responseHeader.set("bones-not-modified", "1");
              return create(method.output);
            }
            return create(method.output, { items: row!.parts.slice(start, end), page } as never);
          }
          groups.push({ widgetId: id, ...(error ? { error } : { items: row!.parts.slice(start, end), page }) });
        }
        if (s.reverse || s.malformed === "order") groups.reverse();
        if (s.malformed === "count") groups.pop();
        return create(method.output, { groups } as never);
      }); } catch (error) {
        if (!(error instanceof ConnectError) || ![Code.NotFound, Code.PermissionDenied, Code.InvalidArgument, Code.FailedPrecondition].includes(error.code)) throw new ConnectError("internal server error", error instanceof ConnectError ? error.code : Code.Internal);
        throw error;
      } finally { context.signal.removeEventListener("abort", aborted); if (batch) { s.active--; } }
    });
  } });
  s.server = createServer(handler);
  await startWidgetsListener(s);
}
export async function startWidgetsListener(s: BatchState): Promise<void> {
  const server = s.server; assert.ok(server);
  if (server.listening) return;
  await new Promise<void>((resolve, reject) => {
    const failed = (error: Error) => reject(error);
    server.once("error", failed);
    server.listen(s.listenerPort ?? 0, "127.0.0.1", () => { server.off("error", failed); resolve(); });
  });
  s.listenerPort = (server.address() as AddressInfo).port;
}
export async function stopWidgetsListener(s: BatchState): Promise<void> {
  const server = s.server; assert.ok(server);
  if (!server.listening) return;
  const closed = new Promise<void>((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
  server.closeAllConnections();
  await closed;
}
export async function storedWidgetOwner(s: BatchState, org: string, id: string): Promise<string | null> {
  const row = (await scopedLookup(s, org, [id])).get(id);
  assert.ok(row, "stored widget exists");
  return row.owner;
}
export async function storedWidgetParts(s: BatchState, org: string, id: string): Promise<{ items: string[]; revision: string }> {
  const row = (await scopedLookup(s, org, [id])).get(id);
  assert.ok(row, "stored widget exists");
  return { items: row.parts, revision: row.revision };
}
export async function lockWidgetReads(s: BatchState): Promise<void> {
  assert.equal(s.readLock, undefined, "one blocking transaction owns the table lock");
  const uri = new URL(postgresAdminUrl); uri.pathname = `/${s.databaseName}`;
  const locker = new pg.Client({ connectionString: uri.toString() });
  await locker.connect();
  try {
    await locker.query("BEGIN");
    await locker.query("LOCK TABLE widgets IN ACCESS EXCLUSIVE MODE");
    s.readLock = locker;
  } catch (error) { await locker.end(); throw error; }
}
export async function waitForBlockedWidgetRead(s: BatchState): Promise<void> {
  assert.ok(s.readLock, "another transaction holds the table lock");
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    await s.readLock.query("SELECT pg_stat_clear_snapshot()");
    const result = await s.readLock.query("SELECT EXISTS(SELECT FROM pg_stat_activity WHERE usename=$1 AND datname=current_database() AND wait_event_type='Lock') AS blocked", [s.databaseRole]);
    if (result.rows[0].blocked) return;
    await delay(10);
  }
  assert.fail("the real PostgreSQL read did not block on the table lock");
}
export async function unlockWidgetReads(s: BatchState): Promise<void> {
  const locker = s.readLock;
  if (!locker) return;
  s.readLock = undefined;
  try { await locker.query("COMMIT"); }
  finally { await locker.end(); }
}
export async function stopWidgetsProvider(s: BatchState) {
 await unlockWidgetReads(s);
 await s.database?.end();
 if (s.databaseName) { const owner = new pg.Client({ connectionString: postgresAdminUrl }); await owner.connect(); try { await owner.query(`DROP DATABASE IF EXISTS ${s.databaseName} WITH (FORCE)`); if (s.databaseRole) await owner.query(`DROP ROLE IF EXISTS ${s.databaseRole}`); } finally { await owner.end(); } }
}
