-- Find every squirrel living in the given tree, newest first.
select s.name, s.acorns_stashed, t.species as tree_species
from squirrels s join trees t on t.id = s.tree_id
where t.id = $1
order by s.born_at desc;
