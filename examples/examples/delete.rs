//! `SqlDelete`: one generated `delete_by_<field>` method per field marked
//! `#[table(delete)]`, plus optional multi-field custom methods.
//!
//! Generic over `sqlx::PgExecutor<'_>`, same as `SqlSelect`/`SqlInsert`/
//! `SqlUpdate` — `&pool` works directly.

use sql_macros::SqlDelete;

#[derive(Debug, SqlDelete)]
#[table(name = "users")]
#[table(delete = delete_inactive(is_active, is_removed))]
pub struct User {
    #[table(delete)]
    pub id: i32,
    pub is_active: bool,
    pub is_removed: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    let rows_affected = User::delete_by_id(&pool, 1).await?.rows_affected();
    println!("{rows_affected}");

    let rows_affected = User::delete_inactive(&pool, false, true)
        .await?
        .rows_affected();
    println!("{rows_affected}");

    Ok(())
}
