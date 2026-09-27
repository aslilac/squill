import Database from "better-sqlite3";

const db = new Database("app.db");

const recentPosts = db.prepare(
  `SELECT p.id, p.title, p.published_at FROM posts p
      WHERE p.author_id = ? AND p.published_at IS NOT NULL
      ORDER BY p.published_at DESC LIMIT 20`,
);

const upsertDraft = db.prepare(`INSERT INTO drafts (post_id, body, saved_at)
    VALUES (@postId, @body, unixepoch()) ON CONFLICT (post_id) DO UPDATE
    SET body = excluded.body`);

export function posts(authorId: number) {
  return recentPosts.all(authorId);
}

export function saveDraft(postId: number, body: string) {
  upsertDraft.run({ postId, body });
}
