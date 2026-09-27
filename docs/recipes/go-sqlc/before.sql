-- name: GetAuthor :one
SELECT * FROM authors
WHERE id = $1 LIMIT 1;

-- name: ListAuthorsByCountry :many
SELECT a.id, a.name, count(b.id) AS book_count FROM authors a
LEFT JOIN books b ON b.author_id = a.id WHERE a.country = @country
GROUP BY a.id ORDER BY book_count DESC LIMIT sqlc.arg(page_size) OFFSET sqlc.arg(page_offset);

-- name: CreateAuthor :one
INSERT INTO authors (name, bio, country) VALUES (@name, sqlc.narg(bio), @country)
RETURNING *;

-- name: DeleteAuthor :exec
DELETE FROM authors WHERE id = $1;
