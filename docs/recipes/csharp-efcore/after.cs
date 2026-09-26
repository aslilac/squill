public partial class AddOrderTotalsView : Migration
{
    protected override void Up(MigrationBuilder migrationBuilder)
    {
        migrationBuilder.Sql(
            """
            create view order_totals as
            select o.id, o.customer_id, sum(l.quantity * l.unit_price) as total
            from orders o join order_lines l on l.order_id = o.id
            group by o.id, o.customer_id
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
            delete from carts
            where
              updated_at < @cutoff
              and not exists (select 1 from orders where orders.cart_id = carts.id)
            """,
            new NpgsqlParameter("cutoff", cutoff));

    public static IQueryable<Order> Late(ShopContext db, int days) =>
        db.Orders.FromSql($"SELECT * FROM orders WHERE shipped_at > placed_at + make_interval(days => {days})");
}
