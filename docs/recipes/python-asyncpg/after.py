async def unread(conn, user_id):
    return await conn.fetch(
        """
        select n.id, n.kind, n.body, n.created_at
        from notifications n
        where n.user_id = $1 and n.read_at is null
        order by n.created_at desc
        limit 100
        """,
        user_id,
    )


async def mark_read(conn, user_id, ids):
    await conn.execute(
        """
        update notifications
        set read_at = now()
        where user_id = $1 and id = any($2::bigint[])
        """,
        user_id,
        ids,
    )
