-- Find every squirrel living in the given tree, newest first.
SELECT s.name, s.acorns_stashed, t.species AS tree_species
FROM squirrels s JOIN trees t ON t.id = s.tree_id WHERE t.id = $1
ORDER BY s.born_at DESC
