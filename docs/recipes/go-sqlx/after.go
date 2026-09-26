package store

func (s *Store) Authors(ctx context.Context, country string) ([]Author, error) {
	var authors []Author
	err := s.db.SelectContext(ctx, &authors, `
	select a.id, a.name, count(b.id) as books
	from authors a left join books b on b.author_id = a.id
	where a.country = $1
	group by a.id, a.name
	order by books desc
	`, country)
	return authors, err
}

func (s *Store) Author(ctx context.Context, id int64) (Author, error) {
	var author Author
	err := s.db.GetContext(ctx, &author, `
	select id, name, country
	from authors
	where id = $1
	`, id)
	return author, err
}

func (s *Store) AddAuthor(ctx context.Context, author Author) error {
	query := s.db.Rebind(`
	insert into authors (name, country)
	values (?, ?)
	on conflict (name) do nothing
	`)
	_, err := s.db.ExecContext(ctx, query, author.Name, author.Country)
	return err
}
