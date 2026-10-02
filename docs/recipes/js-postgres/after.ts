import postgres from "postgres";

const sql = postgres();

export async function setup() {
  await sql`
  create table if not exists events (
    id bigserial primary key,
    kind text not null,
    payload jsonb not null default '{}',
    created_at timestamptz not null default now()
  )
  `;
}

export async function refreshStats() {
  await sql`refresh materialized view concurrently event_stats`;
}

export async function recent(kind: string) {
  return sql`SELECT id, payload, created_at FROM events
      WHERE kind = ${kind} ORDER BY created_at DESC
      LIMIT 50`;
}
