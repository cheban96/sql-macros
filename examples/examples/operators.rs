//! `#[table(op = "...")]`: the comparison a field's own single-field
//! methods use (`select_by_<field>`, `select_many_by_<field>`,
//! `delete_by_<field>`, and `SqlUpdate`'s default `update()`). Defaults to
//! `=` when absent. Custom multi-field methods (`select = name(...)`)
//! never read this — they're always `=`, `AND`-joined (or, for a raw
//! `"$field"` template, whatever SQL the user wrote); see `raw_filter.rs`
//! for those.
//!
//! - `gt`/`lt`/`gte`/`lte` -> `>`, `<`, `>=`, `<=`
//! - `like` -> `LIKE`
//! - `ilike` -> `ILIKE` (Postgres-specific case-insensitive `LIKE`)
//! - `in` -> `= ANY($n)`, and widens the generated parameter from
//!   `FieldType` to `&[FieldType]`
//! - `not_in` -> `!= ALL($n)` — the negation of `in`; also widens the
//!   parameter to `&[FieldType]`
//!
//! `op` also accepts a list — `op = ["gt", "lt"]` — generating one method
//! per operator instead of one field forcing a single fixed comparison
//! everywhere (`select_many_by_id_gt`, `select_many_by_id_lt`, ... — see
//! `UsersByIdRange` below).
//!
//! Filter field names still have to match real column names (there's no
//! `#[table(column = "...")]` override yet), so each operator gets its own
//! struct below rather than piling every op onto one.

use sql_macros::SqlSelectMany;

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersNewerThan {
    pub email: String,
    #[table(select_many, op = "gt")]
    pub id: i32,
}

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersByEmailPattern {
    pub id: i32,
    #[table(select_many, op = "like")]
    pub email: String,
}

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersByEmailPatternCaseInsensitive {
    pub id: i32,
    #[table(select_many, op = "ilike")]
    pub email: String,
}

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersByIdIn {
    pub email: String,
    #[table(select_many, op = "in")]
    pub id: i32,
}

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersByIdNotIn {
    pub email: String,
    #[table(select_many, op = "not_in")]
    pub id: i32,
}

#[derive(Debug, SqlSelectMany)]
#[table(name = "users")]
pub struct UsersByIdRange {
    pub email: String,
    #[table(select_many, op = ["gt", "lt"])]
    pub id: i32,
}

#[tokio::main]
async fn main() -> Result<(), sqlx::Error> {
    let pool = sqlx::PgPool::connect(&std::env::var("DATABASE_URL").unwrap()).await?;

    // Generated as `WHERE id>$1`. Note the method name: a non-`eq` `op`
    // (even a single one, not just a list) is suffixed onto the base name.
    let newer = UsersNewerThan::select_many_by_id_gt(&pool, 0).await?;
    println!("{newer:?}");

    // Generated as `WHERE email LIKE $1`.
    let matches =
        UsersByEmailPattern::select_many_by_email_like(&pool, "%@example.com".into()).await?;
    println!("{matches:?}");

    // Generated as `WHERE email ILIKE $1` — matches regardless of case.
    let matches_ci = UsersByEmailPatternCaseInsensitive::select_many_by_email_ilike(
        &pool,
        "%@EXAMPLE.com".into(),
    )
    .await?;
    println!("{matches_ci:?}");

    // Generated as `WHERE id = ANY($1)`; note the parameter is `&[i32]`,
    // not `i32`, because of `op = "in"`.
    let by_ids = UsersByIdIn::select_many_by_id_in(&pool, &[1, 2, 3]).await?;
    println!("{by_ids:?}");

    // Generated as `WHERE id != ALL($1)` — everyone except ids 1, 2, 3.
    let excluding_ids = UsersByIdNotIn::select_many_by_id_not_in(&pool, &[1, 2, 3]).await?;
    println!("{excluding_ids:?}");

    // `op = ["gt", "lt"]` generates two separate methods instead of one
    // field forcing a single fixed comparison.
    let above = UsersByIdRange::select_many_by_id_gt(&pool, 0).await?; // `WHERE id>$1`
    println!("{above:?}");
    let below = UsersByIdRange::select_many_by_id_lt(&pool, 1000).await?; // `WHERE id<$1`
    println!("{below:?}");

    Ok(())
}
