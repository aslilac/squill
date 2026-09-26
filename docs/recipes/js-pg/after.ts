import type { Pool } from "pg";

export async function openIssues(pool: Pool, projectId: string) {
  const { rows } = await pool.query<Issue>(
    `
    select i.id, i.title, u.name as assignee
    from issues i left join users u on u.id = i.assignee_id
    where i.project_id = $1 and i.closed_at is null
    order by i.priority, i.created_at
    `,
    [projectId],
  );
  return rows;
}

export async function close(pool: Pool, id: string) {
  await pool.query("UPDATE issues SET closed_at = now() WHERE id = $1", [id]);
}
