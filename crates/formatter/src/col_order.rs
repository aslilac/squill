//! Canonical constraint order for a column definition.
//!
//! Postgres accepts a column's constraints in any order; squill emits
//! one: the type, then COLLATE, NOT NULL, the key constraints, and the
//! DEFAULT or GENERATED value last. That is pg_dump's order with NOT
//! NULL pulled up next to the type, and with the inline key constraints
//! — which pg_dump never writes, preferring separate ALTER TABLEs —
//! slotted in ahead of the value, the way `id uuid primary key default
//! gen_random_uuid()` already reads.
//!
//! Same two-view contract as [`crate::attr_order`]: the lowerer permutes
//! the CST elements, the safety oracle permutes the token stream, and
//! both call [`canonical_order`], so a reordered column is still
//! token-equivalent to its input. Where the two views could disagree the
//! permutation is declined, and the statement formats unmoved.

use crate::attr_order::W;

/// Words that open a table constraint. Those share the `ColumnDef` node
/// shape but have no name or type, and nothing in them may move.
const CONSTRAINT_HEADS: &[&str] =
	&["check", "constraint", "exclude", "foreign", "like", "primary", "unique"];

/// The canonical position of a constraint segment, or `None` for a word
/// that continues the one before it.
///
/// Left out on purpose: `deferrable`, `initially`, `on` (a foreign key's
/// actions), `match`, `no` and `not deferrable` all qualify the
/// constraint they follow, so they have to travel with it rather than
/// sort on their own.
fn sort_key(word: &str, next: Option<&str>) -> Option<u8> {
	Some(match word {
		"collate" => 1,
		// `NOT NULL`, and the bare `NULL` that spells out the default.
		// `NOT DEFERRABLE` qualifies the constraint before it.
		"not" if next == Some("null") => 2,
		"null" => 2,
		"primary" if next == Some("key") => 3,
		"unique" => 4,
		"references" => 5,
		"check" => 6,
		"default" | "generated" => 7,
		_ => return None,
	})
}

/// How many units after a segment's first word belong to it whatever
/// they look like. This is what keeps `default null` one segment rather
/// than a DEFAULT and a stray NULL constraint, and it counts units, not
/// words, so the two views agree: a default's argument is one expression
/// node to the lowerer and one or more tokens to the oracle, and either
/// way the first of them is spoken for.
fn forced(word: &str) -> usize {
	match word {
		"not" | "primary" | "collate" | "references" | "default" | "generated" => 1,
		"constraint" => 2,
		_ => 0,
	}
}

/// The next bare word at or after `i`, skipping trivia only.
fn word_at(words: &[W], mut i: usize) -> Option<(usize, &str)> {
	loop {
		match words.get(i)? {
			W::Trivia => i += 1,
			W::Word(text) => return Some((i, text.as_str())),
			_ => return None,
		}
	}
}

/// For one column definition's element sequence, the permutation that
/// puts its constraints in canonical order. `None` when there is nothing
/// to move, or when the sequence is a table constraint rather than a
/// column.
pub(crate) fn canonical_order(words: &[W]) -> Option<Vec<usize>> {
	// A column opens with its name. A table constraint opens with a
	// keyword instead, and is not ours to touch.
	let (start, first) = word_at(words, 0)?;
	if CONSTRAINT_HEADS.contains(&first) {
		return None;
	}

	let mut depth = 0usize;
	let mut starts: Vec<(u8, usize)> = Vec::new();
	let mut skip = 0usize;
	// A referential action spells itself with words that also name
	// constraints: `ON DELETE SET NULL` and `ON DELETE SET DEFAULT` end
	// in one each, and hoisting those out of the foreign key would
	// change what the column means.
	let mut after_set = false;
	let mut i = start + 1;
	while i < words.len() {
		match &words[i] {
			W::Trivia => {
				i += 1;
				continue;
			}
			W::Open => depth += 1,
			W::Close => depth = depth.saturating_sub(1),
			// A comma or semicolon means this is not one column's worth
			// of elements after all; leave the whole thing alone.
			W::Comma | W::Semi if depth == 0 => return None,
			W::Word(word) if depth == 0 && skip == 0 && !after_set => {
				let next = word_at(words, i + 1).map(|(_, w)| w);
				// `CONSTRAINT name <constraint>` sorts as whatever it
				// names, so look past the name for the real word.
				let key = if word == "constraint" {
					Some(
						word_at(words, i + 1)
							.and_then(|(at, _)| word_at(words, at + 1))
							.and_then(|(at, w)| {
								sort_key(w, word_at(words, at + 1).map(|(_, w)| w))
							})
							.unwrap_or(6),
					)
				} else {
					sort_key(word, next)
				};
				// Anything unrecognized continues the segment it is in.
				if let Some(key) = key {
					starts.push((key, i));
					skip = forced(word);
				}
				after_set = word == "set";
				i += 1;
				continue;
			}
			_ => {}
		}
		if depth == 0 && !matches!(words[i], W::Trivia) {
			skip = skip.saturating_sub(1);
			after_set = matches!(&words[i], W::Word(word) if word == "set");
		}
		i += 1;
	}
	if starts.len() < 2 {
		return None;
	}

	// Segment ranges, stably sorted into canonical order.
	let mut ranges: Vec<(u8, std::ops::Range<usize>)> =
		Vec::with_capacity(starts.len());
	for (index, &(key, at)) in starts.iter().enumerate() {
		let stop = starts.get(index + 1).map_or(words.len(), |&(_, next)| next);
		ranges.push((key, at..stop));
	}
	let mut sorted = ranges.clone();
	sorted.sort_by_key(|&(key, _)| key);
	if sorted == ranges {
		return None;
	}
	let mut perm: Vec<usize> = (0..starts[0].1).collect();
	for (_, range) in &sorted {
		perm.extend(range.clone());
	}
	Some(perm)
}
