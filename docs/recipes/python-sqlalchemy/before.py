from sqlalchemy import text


def stale_sessions(conn, cutoff):
    return conn.execute(
        text(
            """SELECT s.id, s.user_id, s.last_seen FROM sessions s WHERE s.last_seen < :cutoff AND NOT s.pinned ORDER BY s.last_seen"""
        ),
        {"cutoff": cutoff},
    ).all()
