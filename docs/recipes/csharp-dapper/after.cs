public sealed class OrderRepository(NpgsqlDataSource db)
{
    public async Task<IEnumerable<OrderSummary>> RecentAsync(int customerId)
    {
        await using var conn = await db.OpenConnectionAsync();
        return await conn.QueryAsync<OrderSummary>(
            """
            select o.id, o.placed_at, sum(l.quantity * l.unit_price) as total
            from orders o join order_lines l on l.order_id = o.id
            where o.customer_id = @customerId
            group by o.id, o.placed_at
            order by o.placed_at desc
            limit 20
            """,
            new { customerId });
    }

    public async Task CancelAsync(int customerId, int[] orderIds)
    {
        await using var conn = await db.OpenConnectionAsync();
        await conn.ExecuteAsync(
            """
            update orders
            set cancelled_at = now()
            where
                customer_id = @customerId
                and id in @orderIds
                and shipped_at is null
            """,
            new { customerId, orderIds });
    }
}
