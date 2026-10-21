//! Field-level `#[table(upsert)]`: each marked field generates its own
//! `upsert_by_<field>` method on `SqlInsert`/`SqlInsertMany` — same shape as
//! `select_by_<field>`/`update_by_<field>`/`delete_by_<field>`: independent,
//! never silently combined with another marked field. That field becomes
//! the `ON CONFLICT (...)` target; every other field becomes a `DO UPDATE
//! SET col = EXCLUDED.col` assignment (or `DO NOTHING` if it's the only
//! field). `insert()`/`insert_many()` themselves are unaffected either way
//! — always a plain `INSERT`, never `ON CONFLICT`.
//!
//! For a single `ON CONFLICT` target spanning more than one column, use a
//! struct-level `#[table(upsert = method_name(field1, field2, ...))]`
//! instead — see `MarkTodoDone` below, which adds an *extra* method
//! alongside the always-present `insert()`.

use sql_macros::{SqlInsert, SqlInsertMany};

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
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated as:
    // "INSERT INTO users (email, is_active) VALUES ($1,$2)
    //  ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active
    //  RETURNING id, email"
    //
    // `&pool` works directly — `SqlInsert`/`SqlInsertMany` are generic over
    // `sqlx::PgExecutor<'_>`, same as every other derive here.
    let user = UpsertUser {
        email: "a@example.com".into(),
        is_active: true,
    }
    .upsert_by_email(&pool)
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
    let users = UpsertUser::upsert_many_by_email(&items, &pool).await?;
    println!("{users:?}");

    // Composite conflict target: seed the user this row references.
    let user_id = user_todo_owner(&pool, "todo-owner@example.com").await?;
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
    let rows_affected = mark.mark_done(&pool).await?.rows_affected();
    println!("{rows_affected}");

    Ok(())
}

/// Inserts (or reuses) a user by email and returns their id, so the
/// `MarkTodoDone` example below has a real `user_id`/`todo_id` pair to
/// upsert against.
async fn user_todo_owner(pool: &sqlx::PgPool, email: &str) -> Result<i32, sqlx::Error> {
    let user = UpsertUser {
        email: email.into(),
        is_active: true,
    }
    .upsert_by_email(pool)
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
