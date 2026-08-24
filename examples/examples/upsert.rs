//! Field-level `#[table(upsert)]`: marks the `ON CONFLICT (...)` target for
//! `SqlInsert`/`SqlInsertMany`. Every other field becomes a `DO UPDATE SET
//! col = EXCLUDED.col` assignment; if every field were part of the target,
//! it would fall back to `DO NOTHING` instead (nothing left to `SET`).

use sql_macros::{SqlInsert, SqlInsertMany};
use sqlx::Connection;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
}

#[derive(Debug, SqlInsert, SqlInsertMany)]
#[table(name = "users", return_type = User, return_fields = "id, email")]
pub struct UpsertUser {
    #[table(upsert)]
    pub email: String,
    pub is_active: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated as:
    // "INSERT INTO users (email, is_active) VALUES ($1,$2)
    //  ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active
    //  RETURNING id, email"
    let user = UpsertUser {
        email: "a@example.com".into(),
        is_active: true,
    }
    .insert(&mut conn)
    .await?;
    println!("{user:?}");

    // Same `ON CONFLICT` clause, applied to every row in the batch.
    let items = vec![
        UpsertUser {
            email: "a@example.com".into(),
            is_active: false,
        },
        UpsertUser {
            email: "b@example.com".into(),
            is_active: true,
        },
    ];
    let users = UpsertUser::insert_many(&items, &mut conn).await?;
    println!("{users:?}");

    Ok(())
}
