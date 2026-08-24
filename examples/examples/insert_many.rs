//! `SqlInsertMany`: batch insert built at runtime with `sqlx::QueryBuilder`
//! (a static `sqlx::query!` can't express a variable number of rows), with
//! the same `return_type`/`return_fields` options as `SqlInsert`.

use sql_macros::SqlInsertMany;
use sqlx::Connection;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
}

#[derive(Debug, SqlInsertMany)]
#[table(name = "users", return_type = User, return_fields = "id, email")]
pub struct CreateUser {
    pub email: String,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // `email` is `UNIQUE`; clean up so this example can be run more than
    // once against the same database.
    sqlx::query!(
        "DELETE FROM users WHERE email IN ($1, $2)",
        "insert-many-a@example.com",
        "insert-many-b@example.com",
    )
    .execute(&mut conn)
    .await?;

    let items = vec![
        CreateUser {
            email: "insert-many-a@example.com".into(),
        },
        CreateUser {
            email: "insert-many-b@example.com".into(),
        },
    ];
    let users = CreateUser::insert_many(&items, &mut conn).await?;
    println!("{users:?}");

    Ok(())
}
