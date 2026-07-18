fn main() {
    let src = r##"
async fn list_workspaces(pool: &PgPool, org: Uuid) -> sqlx::Result<Vec<Workspace>> {
    sqlx::query_as!(
        Workspace,
        r#"SELECT w.id, w.name, u.username AS owner FROM workspaces w JOIN users u ON u.id = w.owner_id WHERE w.organization_id = $1 AND NOT w.deleted ORDER BY w.name"#,
        org
    )
    .fetch_all(pool)
    .await
}

async fn touch(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query!("UPDATE workspaces SET last_used_at=now() WHERE id=$1", id)
        .execute(pool)
        .await?;
    Ok(())
}
"##;
    let out = embed::format_embedded(
        src,
        embed::Host::Rust,
        embed::RUST_SQLX_QUERY,
        &Default::default(),
    )
    .unwrap();
    print!("{out}");
}
