defmodule Shop.Repo.Migrations.AddOrderTotals do
  use Ecto.Migration

  def up do
    execute ~S"""
    create view order_totals as
    select o.id, o.customer_id, sum(li.quantity * li.unit_price) as total
    from orders o join line_items li on li.order_id = o.id
    group by o.id
    """
  end

  def down do
    execute "DROP VIEW order_totals"
  end
end

defmodule Shop.Reports do
  alias Shop.Repo

  def top_customers(since, limit) do
    Repo.query!(
      """
      select c.id, c.email, sum(t.total) as spent
      from customers c join order_totals t on t.customer_id = c.id
      where c.inserted_at > $1
      group by c.id
      order by spent desc
      limit $2
      """,
      [since, limit]
    )
  end

  def by_region(region) do
    Repo.query!("""
    SELECT * FROM customers WHERE region = '#{region}'
    """)
  end
end
