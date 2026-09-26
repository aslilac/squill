defmodule Shop.Repo.Migrations.AddOrderTotals do
  use Ecto.Migration

  def up do
    execute ~S"""
    CREATE VIEW order_totals AS SELECT o.id, o.customer_id, sum(li.quantity * li.unit_price) AS total FROM orders o JOIN line_items li ON li.order_id = o.id GROUP BY o.id
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
      SELECT c.id, c.email, sum(t.total) AS spent FROM customers c JOIN order_totals t ON t.customer_id = c.id WHERE c.inserted_at > $1 GROUP BY c.id ORDER BY spent DESC LIMIT $2
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
