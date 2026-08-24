//! `SqlSelectAll`: a single `select_all` method returning every row, no
//! filters involved.

use sql_macros::SqlSelectAll;

#[derive(Debug, SqlSelectAll)]
#[table(name = "users")]
pub struct AllUsers {
    pub id: i32,
    pub email: String,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    let users = AllUsers::select_all(&pool).await?;
    println!("{users:?}");

    Ok(())
}
