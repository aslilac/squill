public partial class AddOrderTotalsView : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.Sql(
            """
            CREATE VIEW order_totals AS SELECT o.id, o.customer_id, sum(l.quantity * l.unit_price) AS total
                FROM orders o JOIN order_lines l ON l.order_id = o.id
                GROUP BY o.id, o.customer_id
            """);
    }

    protected override void Down(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.Sql("DROP VIEW order_totals");
    }
}

public static class Maintenance
{
    public static Task<int> PurgeAsync(ShopContext db, DateTime cutoff) =>
        db.Database.ExecuteSqlRawAsync(
            """
            DELETE FROM carts WHERE updated_at < @cutoff
                AND NOT EXISTS (SELECT 1 FROM orders
                WHERE orders.cart_id = carts.id)
            """,
            new NpgsqlParameter("cutoff", cutoff));

    public static IQueryable<Order> Late(ShopContext db, int days) =>
        db.Orders.FromSql($"SELECT * FROM orders WHERE shipped_at > placed_at + make_interval(days => {days})");
}
