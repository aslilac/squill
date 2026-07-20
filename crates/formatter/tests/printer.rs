//! TREE-96 acceptance: per-primitive renderer tests and group-breaking
//! behavior at boundary widths, including tab-width sensitivity.

use formatter::IndentStyle;
use formatter::KeywordCase;
use formatter::Options;
use formatter::doc::*;
use formatter::render;

fn opts() -> Options {
	Options::default()
}

#[track_caller]
fn assert_render(doc: &Doc, options: &Options, expected: &str) {
	assert_eq!(render(doc, options), expected);
}

#[test]
fn text_renders_verbatim() {
	assert_render(&text("hello, world"), &opts(), "hello, world");
}

#[test]
fn keyword_case_applies_only_to_keywords() {
	let doc = concat([keyword("SeLeCt"), space(), text("MiXeD")]);
	assert_render(&doc, &opts(), "select MiXeD");
	let upper = Options { keyword_case: KeywordCase::Upper, ..opts() };
	assert_render(&doc, &upper, "SELECT MiXeD");
}

#[test]
fn group_flat_when_it_fits() {
	let doc = group(concat([text("a"), soft_line_or_space(), text("b")]));
	assert_render(&doc, &opts(), "a b");
}

#[test]
fn group_breaks_when_too_wide() {
	let doc = group(concat([
		text("x".repeat(60)),
		soft_line_or_space(),
		text("y".repeat(60)),
	]));
	assert_render(
		&doc,
		&opts(),
		&format!("{}\n{}", "x".repeat(60), "y".repeat(60)),
	);
}

#[test]
fn max_width_option_moves_the_boundary() {
	// Flat layout is 21 chars: fits the default 80, not a width of 20.
	let doc = group(concat([
		text("a".repeat(10)),
		soft_line_or_space(),
		text("b".repeat(10)),
	]));
	assert_render(
		&doc,
		&opts(),
		&format!("{} {}", "a".repeat(10), "b".repeat(10)),
	);
	let narrow = Options { max_width: 20, ..opts() };
	assert_render(
		&doc,
		&narrow,
		&format!("{}\n{}", "a".repeat(10), "b".repeat(10)),
	);
}

#[test]
fn soft_line_is_nothing_when_flat() {
	let doc = group(concat([text("a"), soft_line(), text("b")]));
	assert_render(&doc, &opts(), "ab");
	let doc =
		group(concat([text("x".repeat(50)), soft_line(), text("y".repeat(50))]));
	assert_render(
		&doc,
		&opts(),
		&format!("{}\n{}", "x".repeat(50), "y".repeat(50)),
	);
}

#[test]
fn hard_line_forces_enclosing_group_to_break() {
	let doc = group(concat([
		text("a"),
		soft_line_or_space(),
		text("b"),
		hard_line(),
		text("c"),
	]));
	// Despite being short, the soft line breaks because the group can
	// never be flat.
	assert_render(&doc, &opts(), "a\nb\nc");
}

#[test]
fn indent_uses_tabs_by_default_and_spaces_on_request() {
	let doc = group(concat([
		text("head"),
		indent(concat([hard_line(), text("body")])),
		hard_line(),
		text("tail"),
	]));
	assert_render(&doc, &opts(), "head\n\tbody\ntail");
	let spaces = Options { indent_style: IndentStyle::Spaces, ..opts() };
	assert_render(&doc, &spaces, "head\n  body\ntail");
	let wide_spaces =
		Options { indent_style: IndentStyle::Spaces, indent_width: 4, ..opts() };
	assert_render(&doc, &wide_spaces, "head\n    body\ntail");
}

#[test]
fn if_break_selects_by_group_mode() {
	let flat =
		group(concat([text("a"), if_break(text("<broken>"), text("<flat>"))]));
	assert_render(&flat, &opts(), "a<flat>");
	let broken = group(concat([
		text("x".repeat(90)),
		soft_line(),
		if_break(text("<broken>"), text("<flat>")),
	]));
	assert_render(&broken, &opts(), &format!("{}\n<broken>", "x".repeat(90)));
}

#[test]
fn verbatim_passes_through_and_forces_breaks() {
	let doc = concat([text("before "), verbatim("raw\n  raw2"), text(" after")]);
	assert_render(&doc, &opts(), "before raw\n  raw2 after");
	// A group containing multi-line verbatim always breaks its soft lines.
	let doc = group(concat([text("a"), soft_line_or_space(), verbatim("x\ny")]));
	assert_render(&doc, &opts(), "a\nx\ny");
}

#[test]
fn fill_breaks_only_where_needed() {
	// Ten 18-char words with ", " separators: four fit per 80-col line.
	let words: Vec<String> = (0..6).map(|i| format!("word-{i:0>13}")).collect();
	let mut items = Vec::new();
	for (i, word) in words.iter().enumerate() {
		if i > 0 {
			items.push(concat([text(","), soft_line_or_space()]));
		}
		items.push(text(word.clone()));
	}
	let rendered = render(&fill(items), &opts());
	let lines: Vec<&str> = rendered.lines().collect();
	assert_eq!(lines.len(), 2, "six 18-char words fill two lines: {rendered}");
	let max_width = usize::from(opts().max_width);
	assert!(lines.iter().all(|line| line.chars().count() <= max_width));
	assert!(
		lines[0].contains("word-0000000000000")
			&& lines[0].contains("word-0000000000003")
	);
}

#[test]
fn boundary_exactly_80_fits() {
	// 78 chars + separator-space + 1 char = exactly 80: flat.
	let doc =
		group(concat([text("a".repeat(78)), soft_line_or_space(), text("b")]));
	let rendered = render(&doc, &opts());
	assert!(!rendered.contains('\n'), "exactly 80 cols must stay flat");

	// One more char: 81 breaks.
	let doc =
		group(concat([text("a".repeat(79)), soft_line_or_space(), text("b")]));
	let rendered = render(&doc, &opts());
	assert!(rendered.contains('\n'), "81 cols must break");
}

#[test]
fn trailing_text_after_group_counts_against_the_line() {
	// The group content alone is 79 cols and would fit, but the `,`
	// after the group pushes the line to 81: the group must break.
	let doc = concat([
		group(concat([
			text("a".repeat(39)),
			soft_line_or_space(),
			text("b".repeat(40)),
		])),
		text(","),
	]);
	let rendered = render(&doc, &opts());
	assert!(
		rendered.contains('\n'),
		"group must account for trailing text: {rendered}"
	);
}

#[test]
fn tab_width_changes_measurement() {
	// At indent level 1 the group content is 77 cols wide flat:
	// tab width 2 -> 2 + 77 = 79, fits; tab width 4 -> 4 + 77 = 81, breaks.
	let doc = indent(concat([
		hard_line(),
		group(concat([text("a".repeat(74)), soft_line_or_space(), text("bb")])),
	]));
	let narrow = render(&doc, &opts());
	assert_eq!(
		narrow.lines().count(),
		2,
		"tab width 2 fits on one (indented) line: {narrow:?}"
	);
	let wide = render(&doc, &Options { indent_width: 4, ..opts() });
	assert_eq!(
		wide.lines().count(),
		3,
		"tab width 4 must break the same doc: {wide:?}"
	);
}

#[test]
fn nested_groups_break_outside_in() {
	// Outer breaks, inner still fits flat.
	let inner =
		group(concat([text("inner"), soft_line_or_space(), text("bits")]));
	let doc = group(concat([text("x".repeat(70)), soft_line_or_space(), inner]));
	assert_render(&doc, &opts(), &format!("{}\ninner bits", "x".repeat(70)));
}
