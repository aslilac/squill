import type { Pool } from "pg";

export async function openIssues(pool: Pool, projectId: string) {
  const { rows } = await pool.query<Issue>(
    `SELECT i.id, i.title, u.name AS assignee FROM issues i
        LEFT JOIN users u ON u.id = i.assignee_id WHERE i.project_id = $1
        AND i.closed_at IS NULL ORDER BY i.priority, i.created_at`,
    [projectId],
  );
  return rows;
}

export async function close(pool: Pool, id: string) {
  await pool.query("UPDATE issues SET closed_at = now() WHERE id = $1", [id]);
}
