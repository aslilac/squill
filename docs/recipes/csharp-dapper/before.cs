public sealed class OrderRepository(NpgsqlDataSource db)
{
    public async Task<IEnumerable<OrderSummary>> RecentAsync(int customerId)
    {
        await using var conn = await db.OpenConnectionAsync();
        return await conn.QueryAsync<OrderSummary>(
            """
            SELECT o.id, o.placed_at, sum(l.quantity * l.unit_price) AS total FROM orders o JOIN order_lines l ON l.order_id = o.id WHERE o.customer_id = @customerId GROUP BY o.id, o.placed_at ORDER BY o.placed_at DESC LIMIT 20
            """,
            new { customerId });
    }

    public async Task CancelAsync(int customerId, int[] orderIds)
    {
        await using var conn = await db.OpenConnectionAsync();
        await conn.ExecuteAsync(
            """
            UPDATE orders SET cancelled_at = now() WHERE customer_id=@customerId AND id IN @orderIds AND shipped_at IS NULL
            """,
            new { customerId, orderIds });
    }
}
