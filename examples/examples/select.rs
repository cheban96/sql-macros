//! `SqlSelect`: one generated `select_by_<field>` method per field marked
//! `#[table(select)]`, plus optional multi-field custom methods via
//! `#[table(select = method_name(field1, field2, ...))]`, and `as_type` for
//! columns whose Postgres type needs an explicit cast (e.g. enums).
//!
//! The first parameter is generic over `sqlx::PgExecutor<'_>`, not fixed to
//! `&sqlx::PgPool` — `&pool` still works (below), but so does `&mut *tx`
//! inside a transaction, or `&mut conn` for a single connection.

use sql_macros::SqlSelect;

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "role", rename_all = "snake_case")]
pub enum Role {
    Admin,
    User,
    SuperAdmin,
}

#[derive(Debug, SqlSelect)]
#[table(select = get_active_user(is_active, is_removed))]
pub struct User {
    #[table(select)]
    pub id: i32,
    pub email: String,
    #[table(as_type = "role!: Role")]
    pub role: Role,
    pub is_active: bool,
    pub is_removed: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated from `#[table(select)]` on `id`.
    let by_id = User::select_by_id(&pool, 1).await?;
    println!("{by_id:?}");

    // Generated from `#[table(select = get_active_user(is_active, is_removed))]`.
    let active = User::get_active_user(&pool, true, false).await?;
    println!("{active:?}");

    // Same method, called with `&mut *tx` instead of `&pool` — reads the
    // row as it stands inside the still-open transaction.
    let mut tx = pool.begin().await?;
    let by_id_in_tx = User::select_by_id(&mut *tx, 1).await?;
    println!("{by_id_in_tx:?}");
    tx.commit().await?;

    // And the same method again, called with a bare `&mut PgConnection` —
    // no pool, no transaction, just one connection.
    let mut conn = pool.acquire().await?;
    let by_id_via_conn = User::select_by_id(&mut *conn, 1).await?;
    println!("{by_id_via_conn:?}");

    Ok(())
}
