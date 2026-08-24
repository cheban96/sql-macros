//! A custom multi-field method can also take a raw filter template instead
//! of a plain field list: a single quoted string where `$field` is *only*
//! the bound-value placeholder (numbered by first occurrence; the same
//! field referenced twice reuses one number) — the column name, `=`,
//! `OR`/`AND`/`NOT`, parens, anything else in the string is the user's own
//! SQL, passed through unchanged. `#[table(op = "...")]` on the referenced
//! field is ignored here, since the user already wrote the comparison
//! themselves.

use sql_macros::{SqlSelectMany, SqlUpdate};
use sqlx::Connection;

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
#[table(select_many = search_users(
    "email=$email OR (is_active=$is_active AND NOT is_removed=$is_removed)"
))]
pub struct User {
    pub id: i32,
    pub email: String,
    pub is_active: bool,
    pub is_removed: bool,
}

/// `SqlUpdate` numbers its `WHERE` placeholders *after* its `SET` columns —
/// here `email` is the one `SET` column ($1), so the raw filter's `$id`/
/// `$is_removed` need to come out as $2/$3, not $1/$2. This is the
/// trickiest case for the raw-filter feature (see `render_custom_where`'s
/// `offset` parameter), so it gets its own real compile + run check.
#[derive(Debug, SqlUpdate)]
#[table(name = "users")]
#[table(update = restore_by_id_if_removed("id=$id AND is_removed=$is_removed"))]
pub struct RestoreUser {
    pub email: String,
    #[table(update)]
    pub id: i32,
    pub is_removed: bool,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated as:
    // "SELECT id, email, is_active, is_removed FROM users
    //  WHERE email=$1 OR (is_active=$2 AND NOT is_removed=$3)"
    let matches = User::search_users(&pool, "a@example.com".into(), true, false).await?;
    println!("{matches:?}");

    // Generated as:
    // "UPDATE users SET email=$1 WHERE id=$2 AND is_removed=$3"
    let mut conn = sqlx::PgConnection::connect(&std::env::var("DATABASE_URL").unwrap()).await?;
    let data = RestoreUser {
        email: "restored@example.com".into(),
        id: 1,
        is_removed: true,
    };
    let rows_affected = data.restore_by_id_if_removed(&mut conn).await?.rows_affected();
    println!("{rows_affected}");

    Ok(())
}
