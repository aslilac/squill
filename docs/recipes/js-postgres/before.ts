import postgres from "postgres";

const sql = postgres();

export async function setup() {
  await sql`CREATE TABLE IF NOT EXISTS events (id bigserial PRIMARY KEY, kind text NOT NULL, payload jsonb NOT NULL DEFAULT '{}', created_at timestamptz NOT NULL DEFAULT now())`;
}

export async function refreshStats() {
  await sql`REFRESH MATERIALIZED VIEW CONCURRENTLY event_stats`;
}

export async function recent(kind: string) {
  return sql`SELECT id, payload, created_at FROM events WHERE kind = ${kind} ORDER BY created_at DESC LIMIT 50`;
}
