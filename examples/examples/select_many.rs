//! `SqlSelectMany`: one generated `select_many_by_<field>` method per field
//! marked `#[table(select_many)]`, plus optional multi-field custom methods.

use sql_macros::SqlSelectMany;

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
#[table(select_many = get_users_by_removed(is_active, is_removed))]
pub struct User {
    pub id: i32,
    pub email: String,
    #[table(select_many)]
    pub is_active: bool,
    pub is_removed: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated from `#[table(select_many)]` on `is_active`.
    let active_users = User::select_many_by_is_active(&pool, true).await?;
    println!("{active_users:?}");

    // Generated from `#[table(select_many = get_users_by_removed(is_active, is_removed))]`.
    let removed = User::get_users_by_removed(&pool, true, false).await?;
    println!("{removed:?}");

    Ok(())
}
