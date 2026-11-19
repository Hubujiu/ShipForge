import { createHash, randomBytes, scrypt, timingSafeEqual } from "node:crypto";
import { promisify } from "node:util";
import { z } from "zod";
import { loginSchema, setupSchema, type Actor } from "../protocol";
import { type Db, RequestError, conflict, forbidden, one, pool, transaction } from "./database";

const derive = promisify(scrypt);
const unauthenticated = () => new RequestError(401, "unauthorized", "Sign in to ShipForge");
export const digest = (value: string) => createHash("sha256").update(value).digest("hex");
export const secret = () => randomBytes(32).toString("base64url");

export type Principal =
  | { kind: "member"; actor: Actor; session: string }
  | { kind: "worker"; id: string; orgId: string; ownerId: string };

const actorQuery = `SELECT u.id,u.name,u.username,o.id AS "orgId",o.name AS "orgName",m.role
  FROM members m JOIN users u ON u.id=m.user_id JOIN organizations o ON o.id=m.org_id`;

async function passwordHash(password: string) {
  const salt = randomBytes(24).toString("hex");
  const key = await derive(password, salt, 64) as Buffer;
  return `${salt}:${key.toString("hex")}`;
}

async function matches(password: string, value: string) {
  const [salt, key] = value.split(":");
  if (!salt || !key || key.length !== 128) return false;
  const checked = await derive(password, salt, 64) as Buffer;
  return timingSafeEqual(checked, Buffer.from(key, "hex"));
}

async function session(db: Db, userId: string, orgId: string) {
  const token = secret();
  await db.query("INSERT INTO sessions(digest,org_id,user_id) VALUES($1,$2,$3)", [digest(token), orgId, userId]);
  const actor = await one<Actor>(db, `${actorQuery} WHERE u.id=$1 AND o.id=$2`, [userId, orgId]);
  return { token, actor };
}

export async function setup(input: unknown, remoteAddress: string | undefined) {
  const data = setupSchema.parse(input);
  const expected = process.env.SHIPFORGE_BOOTSTRAP_KEY;
  if (expected ? digest(data.bootstrapKey ?? "") !== digest(expected) : !["127.0.0.1", "::1", "::ffff:127.0.0.1"].includes(remoteAddress ?? "")) {
    throw forbidden("A deployment bootstrap key is required");
  }
  const hash = await passwordHash(data.password);
  return transaction(async (db) => {
    await db.query("SELECT pg_advisory_xact_lock(5132473063)");
    if (await one(db, "SELECT 1 FROM organizations LIMIT 1")) throw conflict("The workspace has already been initialized");
    const actor = await one<{ id: string }>(db, "INSERT INTO users(username,name,password_hash) VALUES($1,$2,$3) RETURNING id",
      [data.username, data.name, hash]);
    const org = await one<{ id: string }>(db, "INSERT INTO organizations(name) VALUES($1) RETURNING id", [data.organization]);
    await db.query("INSERT INTO members(org_id,user_id,role) VALUES($1,$2,'owner')", [org!.id, actor!.id]);
    await db.query("INSERT INTO spaces(org_id,name) VALUES($1,$2)", [org!.id, data.firstSpace]);
    return session(db, actor!.id, org!.id);
  });
}

export async function login(input: unknown) {
  const data = loginSchema.parse(input);
  const entry = await one<{ id: string; password_hash: string; org_id: string }>(pool,
    "SELECT u.id,u.password_hash,m.org_id FROM users u JOIN members m ON m.user_id=u.id WHERE username=$1 ORDER BY m.org_id LIMIT 1",
    [data.username]);
  const hash = entry?.password_hash ?? `${"0".repeat(48)}:${"0".repeat(128)}`;
  if (!await matches(data.password, hash) || !entry) throw unauthenticated();
  return transaction((db) => session(db, entry.id, entry.org_id));
}

export async function acceptInvitation(input: unknown) {
  const data = loginSchema.extend({ name: z.string().trim().min(1).max(160), code: z.string().min(32).max(128) }).parse(input);
  const hash = await passwordHash(data.password);
  return transaction(async (db) => {
    const invitation = await one<{org_id: string; role: string}>(db, `UPDATE invitations SET consumed_at=now()
      WHERE digest=$1 AND consumed_at IS NULL AND expires_at>now() RETURNING org_id,role`, [digest(data.code)]);
    if (!invitation) throw forbidden("This invitation is expired or has been used");
    const user = await one<{ id: string }>(db, "INSERT INTO users(username,name,password_hash) VALUES($1,$2,$3) RETURNING id",
      [data.username, data.name, hash]);
    await db.query("INSERT INTO members(org_id,user_id,role) VALUES($1,$2,$3)", [invitation.org_id, user!.id, invitation.role]);
    return session(db, user!.id, invitation.org_id);
  });
}

export async function authenticate(authorization?: string): Promise<Principal> {
  const raw = /^Bearer ([A-Za-z0-9_-]{30,128})$/i.exec(authorization ?? "")?.[1];
  if (!raw) throw unauthenticated();
  const hashed = digest(raw);
  const actor = await one<Actor>(pool, `${actorQuery} JOIN sessions s ON s.org_id=m.org_id AND s.user_id=m.user_id
    WHERE s.digest=$1 AND s.expires_at>now()`, [hashed]);
  if (actor) return { kind: "member", actor, session: hashed };
  const worker = await one<{ id: string; orgId: string; ownerId: string }>(pool,
    'SELECT id,org_id AS "orgId",owner_id AS "ownerId" FROM machines WHERE worker_digest=$1', [hashed]);
  if (worker) return { kind: "worker", ...worker };
  throw unauthenticated();
}

export async function logout(principal: Principal) {
  if (principal.kind !== "member") throw forbidden();
  await pool.query("DELETE FROM sessions WHERE digest=$1", [principal.session]);
}
