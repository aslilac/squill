async def unread(conn, user_id):
    return await conn.fetch(
        """SELECT n.id, n.kind, n.body, n.created_at
            FROM notifications n WHERE n.user_id = $1
            AND n.read_at IS NULL ORDER BY n.created_at DESC
            LIMIT 100""",
        user_id,
    )


async def mark_read(conn, user_id, ids):
    await conn.execute(
        """UPDATE notifications SET read_at = now()
            WHERE user_id = $1 AND id = any($2::bigint[])""",
        user_id,
        ids,
    )
