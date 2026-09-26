pub async fn active_members(pool: &PgPool, team: Uuid) -> sqlx::Result<Vec<Member>> {
    sqlx::query_as!(
        Member,
        r#"SELECT m.id, m.name, m.role AS "role: Role" FROM members m JOIN teams t ON t.id = m.team_id WHERE t.id = $1 AND m.deactivated_at IS NULL ORDER BY m.name"#,
        team,
    )
    .fetch_all(pool)
    .await
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE members
         SET name = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn count(pool: &PgPool) -> sqlx::Result<i64> {
    sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM members")
        .fetch_one(pool)
        .await
}
