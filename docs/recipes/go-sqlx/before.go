package store

func (s *Store) Authors(ctx context.Context, country string) ([]Author, error) {
	var authors []Author
	err := s.db.SelectContext(ctx, &authors, `SELECT a.id, a.name, count(b.id) AS books
		FROM authors a LEFT JOIN books b ON b.author_id = a.id
		WHERE a.country = $1 GROUP BY a.id, a.name ORDER BY books DESC`, country)
	return authors, err
}

func (s *Store) Author(ctx context.Context, id int64) (Author, error) {
	var author Author
	err := s.db.GetContext(ctx, &author, `SELECT id, name, country
		FROM authors WHERE id = $1`, id)
	return author, err
}

func (s *Store) AddAuthor(ctx context.Context, author Author) error {
	query := s.db.Rebind(`INSERT INTO authors (name, country)
		VALUES (?, ?) ON CONFLICT (name) DO NOTHING`)
	_, err := s.db.ExecContext(ctx, query, author.Name, author.Country)
	return err
}
