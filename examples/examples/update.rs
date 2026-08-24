//! `SqlUpdate`: a `#[table(update)]` field generates its own
//! `update_by_<field>` method — that field is the `WHERE` filter, every
//! other field becomes a `SET` column (same shape as
//! `select_by_<field>`/`delete_by_<field>`; two different `#[table(update)]`
//! fields never combine into one method). `spec_columns` appends raw SQL to
//! the `SET` clause (e.g. `updated_at = NOW()`), and
//! `#[table(update = method_name(field, ...))]` generates additional,
//! differently-filtered update methods on the same struct.

use sql_macros::SqlUpdate;
use sqlx::Connection;

#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
}

#[derive(Debug, SqlUpdate)]
#[table(
    name = "users",
    return_type = User,
    return_fields = "id, email",
    spec_columns = "updated_at=NOW()",
    update = update_by_email("email=$email")
)]
pub struct UpdateUser {
    #[table(update)]
    pub id: i32,
    pub email: String,
    pub is_active: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    let data = UpdateUser {
        id: 1,
        email: "new@example.com".into(),
        is_active: false,
    };

    // `update_by_id`: filters on `id` (marked `#[table(update)]`), sets
    // `email`, `is_active`, plus `updated_at=NOW()` from `spec_columns`.
    let user = data.update_by_id(&mut conn).await?;
    println!("{user:?}");

    // `update_by_email`: filters on `email` instead, sets `id`, `is_active`.
    let user = data.update_by_email(&mut conn).await?;
    println!("{user:?}");

    Ok(())
}
