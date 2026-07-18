//! Test fixture: sqlx-style query macros over Postgres SQL. The macros
//! are local shims so `cargo check` runs offline with no database; the
//! call shapes match real sqlx.

#[macro_export]
macro_rules! query {
    ($($t:tt)*) => {
        ()
    };
}

#[macro_export]
macro_rules! query_as {
    ($($t:tt)*) => {
        ()
    };
}

#[macro_export]
macro_rules! query_scalar {
    ($($t:tt)*) => {
        ()
    };
}

pub mod sqlx {
    pub use crate::{query, query_as, query_scalar};
}

pub struct User;

pub fn queries() {
    let _ = sqlx::query!(
        "SELECT id, name, created_at FROM users WHERE org_id = $1 AND deleted = false ORDER BY created_at DESC LIMIT $2"
    );
    let _ = sqlx::query_as!(
        User,
        r#"select u.id,count(*) AS n from users u join orgs o on o.id=u.org_id where o.active group by u.id"#
    );
    let _ = sqlx::query_scalar!("SELECT   count(*) FROM api_keys WHERE user_id = $1");
    // Not SQL: must stay byte-identical.
    let _ = sqlx::query!("{not sql at all}");
    // Not a query macro: must stay byte-identical.
    let _ = format!("SELECT   * FROM decoy");
}
