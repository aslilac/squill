package store

func (s *Store) OpenOrders(ctx context.Context, customerID int64) ([]Order, error) {
	rows, err := s.db.QueryContext(
		ctx,
		`SELECT o.id, o.total_cents, o.created_at FROM orders o
			WHERE o.customer_id = $1 AND o.status = 'open' ORDER BY o.created_at DESC`,
		customerID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	return scanOrders(rows)
}

func (s *Store) Archive(ctx context.Context, before time.Time) (int64, error) {
	tag, err := s.pool.Exec(
		ctx,
		"INSERT INTO orders_archive SELECT * FROM orders\nWHERE created_at < $1 AND status IN ('delivered', 'cancelled')",
		before,
	)
	return tag.RowsAffected(), err
}

func (s *Store) Cancel(ctx context.Context, id int64) error {
	_, err := s.db.ExecContext(ctx, "UPDATE orders SET status = 'cancelled' WHERE id = $1", id)
	return err
}
