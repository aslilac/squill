def top_customers(conn, since, limit=10):
    with conn.cursor() as cur:
        cur.execute(
            """SELECT c.id, c.name, sum(o.total) AS spent FROM customers c JOIN orders o ON o.customer_id = c.id WHERE o.placed_at >= %(since)s GROUP BY c.id, c.name ORDER BY spent DESC LIMIT %(limit)s""",
            {"since": since, "limit": limit},
        )
        return cur.fetchall()


def tag_orders(conn, tagged):
    with conn.cursor() as cur:
        cur.executemany(
            """
            INSERT INTO order_tags (order_id, tag) VALUES (%s, %s)
            ON CONFLICT DO NOTHING
            """,
            tagged,
        )


def order_count(conn):
    with conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM orders")
        return cur.fetchone()[0]
