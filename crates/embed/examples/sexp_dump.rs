fn dump(lang: tree_sitter::Language, name: &str, src: &str) {
    let mut p = tree_sitter::Parser::new();
    p.set_language(&lang).unwrap();
    let tree = p.parse(src, None).unwrap();
    println!("=== {name}\n{}", tree.root_node().to_sexp());
}
fn main() {
    dump(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        "cur.execute('''SELECT 1''')\nq = text(\"SELECT 2\")\nf = f'{x}'\n",
    );
    dump(
        tree_sitter_javascript::LANGUAGE.into(),
        "js",
        "db.query(`SELECT 1`); const r = sql`SELECT ${x}`; p.execute('SELECT 3');\n",
    );
    dump(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "ts",
        "const a: number = await db.query(`SELECT 1`);\n",
    );
    dump(
        tree_sitter_gleam::LANGUAGE.into(),
        "gleam",
        "pub fn go(db) {\n  sqlight.query(\"select 1\", on: db, with: [])\n  pog.query(\"select 2\")\n}\n",
    );
}
