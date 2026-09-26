import Database from "better-sqlite3";

const db = new Database("app.db");

const recentPosts = db.prepare(
  `
  select p.id, p.title, p.published_at
  from posts p
  where p.author_id = ? and p.published_at is not null
  order by p.published_at desc
  limit 20
  `,
);

const upsertDraft = db.prepare(`
insert into drafts (post_id, body, saved_at)
values (@postId, @body, unixepoch())
on conflict (post_id) do update set body = excluded.body
`);

export function posts(authorId: number) {
  return recentPosts.all(authorId);
}

export function saveDraft(postId: number, body: string) {
  upsertDraft.run({ postId, body });
}
