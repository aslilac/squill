def top_customers(conn, since, limit=10):
    with conn.cursor() as cur:
        cur.execute(
            """
            select c.id, c.name, sum(o.total) as spent
            from customers c join orders o on o.customer_id = c.id
            where o.placed_at >= %(since)s
            group by c.id, c.name
            order by spent desc
            limit %(limit)s
            """,
            {"since": since, "limit": limit},
        )
        return cur.fetchall()


def tag_orders(conn, tagged):
    with conn.cursor() as cur:
        cur.executemany(
            """
            insert into order_tags (order_id, tag)
            values (%s, %s)
            on conflict do nothing
            """,
            tagged,
        )


def order_count(conn):
    with conn.cursor() as cur:
        cur.execute("SELECT count(*) FROM orders")
        return cur.fetchone()[0]
