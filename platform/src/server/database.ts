import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { Pool, type PoolClient, type QueryResultRow } from "pg";

export const eventsChannel = "shipforge_workspace";
export class RequestError extends Error {
  constructor(public readonly status: number, public readonly code: string, message: string) {
    super(message);
  }
}

export type Db = Pick<PoolClient, "query">;
export const pool = new Pool({
  connectionString: process.env.DATABASE_URL,
  max: 12,
  connectionTimeoutMillis: 5000,
  statement_timeout: 15_000,
  idle_in_transaction_session_timeout: 20_000,
});
pool.on("error", () => console.error("A database connection was interrupted"));

export function transaction<T>(action: (db: Db) => Promise<T>, mode = ""): Promise<T> {
  return pool.connect().then(async (db) => {
    try {
      await db.query(mode === "read" ? "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY" : "BEGIN");
      const result = await action(db);
      await db.query("COMMIT");
      return result;
    } catch (error) {
      await db.query("ROLLBACK").catch(() => undefined);
      throw error;
    } finally {
      db.release();
    }
  });
}

export async function one<T extends QueryResultRow>(db: Db, sql: string, fields: unknown[] = []) {
  return (await db.query<T>(sql, fields)).rows[0] as T | undefined;
}

export async function rows<T extends QueryResultRow>(db: Db, sql: string, fields: unknown[] = []) {
  return (await db.query<T>(sql, fields)).rows;
}

export async function changed(db: Db, orgId: string, actorId?: string) {
  await db.query("SELECT pg_notify($1,$2)", [eventsChannel, JSON.stringify({ orgId, actorId })]);
}

export async function migrate() {
  const sql = await readFile(join(__dirname, "schema.sql"), "utf8");
  await transaction(async (db) => {
    await db.query("SELECT pg_advisory_xact_lock(5132473062)");
    await db.query(sql);
  });
}

export const missing = (message = "Record is unavailable") => new RequestError(404, "not_found", message);
export const forbidden = (message = "This account cannot perform the operation") => new RequestError(403, "forbidden", message);
export const conflict = (message: string) => new RequestError(409, "conflict", message);
export const invalid = (message: string) => new RequestError(400, "invalid", message);
