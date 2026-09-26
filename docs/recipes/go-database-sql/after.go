package store

func (s *Store) OpenOrders(ctx context.Context, customerID int64) ([]Order, error) {
	rows, err := s.db.QueryContext(
		ctx,
		`
		select o.id, o.total_cents, o.created_at
		from orders o
		where o.customer_id = $1 and o.status = 'open'
		order by o.created_at desc
		`,
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
		`
		insert into orders_archive
		select *
		from orders
		where created_at < $1 and status in ('delivered', 'cancelled')
		`,
		before,
	)
	return tag.RowsAffected(), err
}

func (s *Store) Cancel(ctx context.Context, id int64) error {
	_, err := s.db.ExecContext(ctx, "UPDATE orders SET status = 'cancelled' WHERE id = $1", id)
	return err
}
