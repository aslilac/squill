from sqlalchemy import text


def stale_sessions(conn, cutoff):
    return conn.execute(
        text(
            """
            select s.id, s.user_id, s.last_seen
            from sessions s
            where s.last_seen < :cutoff and not s.pinned
            order by s.last_seen
            """
        ),
        {"cutoff": cutoff},
    ).all()
