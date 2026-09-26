-- name: GetAuthor :one
select * from authors where id = $1 limit 1;

-- name: ListAuthorsByCountry :many
select a.id, a.name, count(b.id) as book_count
from authors a left join books b on b.author_id = a.id
where a.country = @country
group by a.id
order by book_count desc
limit sqlc.arg(page_size)
offset sqlc.arg(page_offset);

-- name: CreateAuthor :one
insert into authors (name, bio, country)
values (@name, sqlc.narg(bio), @country)
returning *;

-- name: DeleteAuthor :exec
delete from authors where id = $1;
