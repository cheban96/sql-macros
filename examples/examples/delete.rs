//! `SqlDelete`: one generated `delete_by_<field>` method per field marked
//! `#[table(delete)]`, plus optional multi-field custom methods.

use sql_macros::SqlDelete;
use sqlx::Connection;

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
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    let rows_affected = User::delete_by_id(&mut conn, 1).await?.rows_affected();
    println!("{rows_affected}");

    let rows_affected = User::delete_inactive(&mut conn, false, true)
        .await?
        .rows_affected();
    println!("{rows_affected}");

    Ok(())
}
