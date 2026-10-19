//! Field-level `#[table(upsert)]`: marks the `ON CONFLICT (...)` target for
//! `SqlInsert`/`SqlInsertMany`. Every other field becomes a `DO UPDATE SET
//! col = EXCLUDED.col` assignment; if every field were part of the target,
//! it would fall back to `DO NOTHING` instead (nothing left to `SET`).
//!
//! At most one field may carry the bare flag — marking two is a compile
//! error, since there's no way to tell from two bare flags whether you mean
//! a composite target or two independent ones. For a composite target
//! spanning several columns, use a struct-level `#[table(upsert =
//! method_name(field1, field2, ...))]` instead — see `MarkTodoDone` below,
//! which adds an *extra* method alongside the always-present `insert()`.

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

    // Composite conflict target: seed the user this row references.
    let user_id = user_todo_owner(&mut conn, "todo-owner@example.com").await?;
    let mark = MarkTodoDone {
        user_id,
        todo_id: 1,
        done: true,
    };

    // Generated as:
    // "INSERT INTO user_todos (user_id, todo_id, done) VALUES ($1,$2,$3)
    //  ON CONFLICT (user_id, todo_id) DO UPDATE SET done=EXCLUDED.done"
    //
    // `mark_done` is *additional* — `MarkTodoDone` also still has a plain
    // `insert()` (no `ON CONFLICT` at all), since no field here carries the
    // bare `#[table(upsert)]` flag.
    let rows_affected = mark.mark_done(&mut conn).await?.rows_affected();
    println!("{rows_affected}");

    Ok(())
}

/// Inserts (or reuses) a user by email and returns their id, so the
/// `MarkTodoDone` example below has a real `user_id`/`todo_id` pair to
/// upsert against.
async fn user_todo_owner(conn: &mut sqlx::PgConnection, email: &str) -> Result<i32, sqlx::Error> {
    let user = UpsertUser {
        email: email.into(),
        is_active: true,
    }
    .insert(conn)
    .await?;
    Ok(user.id)
}

/// Composite `ON CONFLICT (user_id, todo_id)` — needs both columns to
/// identify a conflict, so a bare `#[table(upsert)]` on just one of them
/// wouldn't match the real `UNIQUE (user_id, todo_id)` constraint.
#[derive(Debug, SqlInsert)]
#[table(
    name = "user_todos",
    upsert = mark_done(user_id, todo_id)
)]
pub struct MarkTodoDone {
    pub user_id: i32,
    pub todo_id: i32,
    pub done: bool,
}
