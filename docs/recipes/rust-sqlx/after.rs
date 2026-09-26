pub async fn active_members(pool: &PgPool, team: Uuid) -> sqlx::Result<Vec<Member>> {
    sqlx::query_as!(
        Member,
        r#"
        select m.id, m.name, m.role as "role: Role"
        from members m join teams t on t.id = m.team_id
        where t.id = $1 and m.deactivated_at is null
        order by m.name
        "#,
        team,
    )
    .fetch_all(pool)
    .await
}

pub async fn rename(pool: &PgPool, id: Uuid, name: &str) -> sqlx::Result<()> {
    sqlx::query(
        r#"
        update members
        set name = $2, updated_at = now()
        where id = $1
        "#,
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
