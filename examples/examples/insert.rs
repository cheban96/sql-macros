//! `SqlInsert`: a single-row `insert(&self, conn)` method. Three flavors,
//! selected by which attributes are present:
//! - no `return_type` -> returns `sqlx::any::AnyQueryResult`
//! - `return_type = User` -> `RETURNING *`, fetched into `User`
//! - `return_type = ... , return_fields = "id"` -> `RETURNING id` only
//!
//! A field with `#[table(as_type = "...")]` (a Postgres enum/domain column)
//! binds as `self.field as _` in the generated `INSERT`; every other field
//! binds as a bare `self.field`, so `sqlx::query!`'s normal compile-time
//! type check still applies to it — see `CreateUserWithRole` below.
//!
//! `return_type` without `return_fields` is `RETURNING *`, which can't
//! decode an enum *return* column without help (the macro never sees
//! `UserWithRole`'s own fields, only its type name) — write the `AS "col!:
//! Type"` override into `return_fields` yourself; see
//! `CreateUserReturningRole` below and the README's "Attention" section.

use sql_macros::SqlInsert;
use sqlx::Connection;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
}

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "role", rename_all = "snake_case")]
pub enum Role {
    Admin,
    User,
    SuperAdmin,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users")]
pub struct CreateUserWithRole {
    pub email: String,
    #[table(as_type = "role!: Role")]
    pub role: Role,
}

/// `return_type` without `return_fields` generates `RETURNING *`, which
/// can't carry the `AS "role!: Role"` override `sqlx::query_as!` needs to
/// decode an enum column — the macro only sees `return_type` as a type
/// name, not `UserWithRole`'s own field list, so it can't add the
/// annotation for you. Spell it out yourself in `return_fields` instead
/// (it's inserted into the query as plain text, so a type override in
/// there works fine) — see the README's "Attention" section.
#[derive(Debug, sqlx::FromRow)]
pub struct UserWithRole {
    pub id: i32,
    pub email: String,
    pub role: Role,
}

#[derive(Debug, SqlInsert)]
#[table(
    name = "users",
    return_type = UserWithRole,
    return_fields = "id, email, role AS \"role!: Role\""
)]
pub struct CreateUserReturningRole {
    pub email: String,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users")]
pub struct CreateUser {
    pub email: String,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = User, return_fields = "id, email")]
pub struct CreateUserReturning {
    pub email: String,
}

#[derive(Debug, sqlx::FromRow)]
pub struct CreateUserId {
    pub id: i32,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = CreateUserId, return_fields = "id")]
pub struct CreateUserReturningId {
    pub email: String,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // `email` is `UNIQUE`; clean up so this example can be run more than
    // once against the same database.
    sqlx::query!(
        "DELETE FROM users WHERE email IN ($1, $2, $3, $4, $5)",
        "insert-a@example.com",
        "insert-b@example.com",
        "insert-c@example.com",
        "insert-role@example.com",
        "insert-role-returning@example.com",
    )
    .execute(&mut conn)
    .await?;

    let rows_affected = CreateUserWithRole {
        email: "insert-role@example.com".into(),
        role: Role::Admin,
    }
    .insert(&mut conn)
    .await?
    .rows_affected();
    println!("{rows_affected}");

    let user = CreateUserReturningRole {
        email: "insert-role-returning@example.com".into(),
    }
    .insert(&mut conn)
    .await?;
    println!("{user:?}");

    let rows_affected = CreateUser {
        email: "insert-a@example.com".into(),
    }
    .insert(&mut conn)
    .await?
    .rows_affected();
    println!("{rows_affected}");

    let user = CreateUserReturning {
        email: "insert-b@example.com".into(),
    }
    .insert(&mut conn)
    .await?;
    println!("{user:?}");

    let created_id = CreateUserReturningId {
        email: "insert-c@example.com".into(),
    }
    .insert(&mut conn)
    .await?;
    println!("{created_id:?}");

    Ok(())
}
