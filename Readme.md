<!-- markdownlint-disable MD033 -->

# Crate sql-macros

`sql-macros` - it's simple lib for generate sql query for select, select_many, select_all, insert, update, delete

## 0.2 changes

The attribute parser was rewritten from a hand-rolled `TokenTree` walker onto
[`darling`](https://crates.io/crates/darling), on `syn` 3.0. This fixes a
couple of real bugs (field types with 0 or 2+ generic arguments used to be
silently dropped during codegen; `SqlInsertMany` didn't actually batch-insert
anything) and replaces `panic!`s with span-accurate compile errors.

**Breaking changes:**

- `#[table(name = users)]` (bare identifier) is no longer accepted — write
  `#[table(name = "users")]` instead.
- `SqlUpdate` no longer generates a single default `update()` combining
  every `#[table(update)]` field into one `WHERE` clause. Each
  `#[table(update)]` field now generates its own `update_by_<field>` method
  instead — same shape as `select_by_<field>`/`delete_by_<field>` — so
  `data.update(conn)` becomes `data.update_by_id(conn)` (or whichever field
  you marked). Two different `#[table(update)]` fields never combine into
  one method anymore.

Everything else — `return_type = User`, `select = method(field1, field2)`,
`as_type = "..."`, `spec_columns = "..."`, `return_fields = "..."` — is
unchanged.

New in 0.2:

- `SqlInsertMany` now does a real multi-row `INSERT ... VALUES (...),
  (...), ...` via `sqlx::QueryBuilder`, instead of silently discarding the
  query result — see [Insert many](#insert-many).
- Upsert — `#[table(upsert)]` on a field generates `ON CONFLICT (...) DO
  UPDATE`/`DO NOTHING` for `SqlInsert`/`SqlInsertMany`; a struct-level
  `#[table(upsert = method_name(field1, field2, ...))]` generates an
  additional method with a composite conflict target — see
  [Upsert](#upsert).
- Comparison operators — `#[table(op = "gt"|"lt"|"gte"|"lte"|"like"|"ilike"|
  "in"|"not_in")]` on a filter field, single value or a list (one method
  variant per operator) — see [Comparison operators](#comparison-operators).
- Custom multi-field methods (`#[table(select = name(fields))]` and the
  same for `select_many`/`update`/`delete`) can now also take a raw
  `"$field"`-templated string instead of a plain field list, for `OR`/`NOT`/
  anything beyond a plain `AND` of equalities — see
  [Custom multi-field methods](#custom-multi-field-methods) and
  [Generate methods with many fields](#generate-methods-with-many-fields).

## 0.3.0 changes

**Breaking change:**

- Marking two or more fields bare `#[table(upsert)]` used to silently
  combine them into one composite `ON CONFLICT (...)` target. That's now a
  compile error — at most one field may carry the bare flag. For a
  composite target spanning several columns, use the new struct-level
  `#[table(upsert = method_name(field1, field2, ...))]` attribute instead,
  which generates an *additional*, separately named method alongside the
  always-present `insert()`/`insert_many()` — see [Upsert](#upsert).

Also new in 0.3.0:

- `SqlSelect`/`SqlSelectAll`/`SqlSelectMany` methods take
  `impl sqlx::PgExecutor<'_>` instead of a fixed `&sqlx::PgPool`. Not
  breaking — every existing call site passing `&pool` keeps working — but
  now `&mut *tx` (inside a `sqlx::Transaction`) or `&mut conn` (a bare
  `PgConnection`, e.g. from `pool.acquire()`) works too.

## 0.4.0 changes

**Breaking change:**

- Bare `#[table(upsert)]` no longer modifies the shared `insert()`/
  `insert_many()` method. Each marked field now generates its *own*
  independent method instead — `upsert_by_<field>` for `SqlInsert`,
  `upsert_many_by_<field>` for `SqlInsertMany` — same shape as
  `select_by_<field>`/`update_by_<field>`/`delete_by_<field>`: a marked
  field is never silently combined with another one. Marking two (or more)
  fields bare is no longer a compile error either — each just gets its own
  method. `insert()`/`insert_many()` themselves are now *always* a plain
  `INSERT`, never `ON CONFLICT`, regardless of how many fields are marked.
  This supersedes 0.3.0's single-bare-field behavior (`insert()` itself
  carrying `ON CONFLICT`) and its "2+ bare fields is a compile error" rule
  — see [Upsert](#upsert).
- Fixed: a struct-level `#[table(upsert = method_name(...))]` on a struct
  deriving both `SqlInsert` and `SqlInsertMany` used to generate the same
  method name for both (`method_name`), which fails to compile
  (`duplicate definitions with name ...`) the moment both derives are
  actually present together. `SqlInsertMany` now names its copy
  `method_name_many` instead — same `insert`/`insert_many` split used
  everywhere else — so the two coexist.

Also new in 0.4.0:

- `SqlInsert`/`SqlInsertMany`/`SqlUpdate`/`SqlDelete` methods now also take
  `impl sqlx::PgExecutor<'_>`, same as the select-side derives got in 0.3.0.
  Not breaking on its own — `&mut conn`/`&mut *tx` call sites keep working —
  but these write-side methods, which used to require `&mut
  sqlx::PgConnection` specifically, now also accept `&pool` directly,
  without pre-acquiring a connection first.

## Install

```toml
# Cargo.toml
[dependencies]
sql-macros = { version = "0.4" }
```

## Usage

## Table name

```rust
use sql_macros::SqlSelect;

#[derive(SqlSelect)]
pub struct User {
    #[table(select)]
    pub id: i32,
    pub email: String,
}
```

Table name will be generated as `users`

If you need use special name use `#[table(name = "users")]`

## Select one

Every `SqlSelect`/`SqlSelectAll`/`SqlSelectMany` method takes its first
parameter as `impl sqlx::PgExecutor<'_>`, not a fixed `&sqlx::PgPool` — a
plain `&pool` still works everywhere below, but so does `&mut *tx` inside a
transaction, or `&mut conn` for a single connection.

```rust
use sql_macros::SqlSelect;

#[derive(SqlSelect)]
pub struct User {
    #[table(select)]
    pub id: i32,
    pub email: String,
}

pub async fn get_by_id(pool: &sqlx::PgPool, id: i32) -> Result<Option<User>, sqlx::Error> {
    let user = User::select_by_id(pool, id).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT id, email FROM users WHERE id=$1"]
    pub async fn select_by_id<'e, E>(executor: E, id: i32) -> Result<Option<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT id, email FROM users WHERE id=$1", id)
            .fetch_optional(executor)
            .await?;
        Ok(object)
    }
}
```

</details>

## Select all

```rust
use sql_macros::SqlSelectAll;

#[derive(SqlSelectAll)]
pub struct User {
    pub id: i32,
    pub email: String,
}
pub async fn get_all_users(pool: &sqlx::PgPool) -> Result<Vec<User>, sqlx::Error> {
    let users = User::select_all(pool).await?;
    Ok(users)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT id, email FROM users"]
    pub async fn select_all<'e, E>(executor: E) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT id, email FROM users")
            .fetch_all(executor)
            .await?;
        Ok(object)
    }
}
```

</details>

## Select many

```rust
use sql_macros::SqlSelectMany;

#[derive(SqlSelectMany)]
pub struct User {
    pub id: i32,
    pub email: String,
    #[table(select_many)]
    pub is_removed: bool,
}
pub async fn get_by_removed(pool: &sqlx::PgPool, is_removed: bool) -> Result<Vec<User>, sqlx::Error> {
    let users = User::select_many(pool, is_removed).await?;
    Ok(users)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT id, email, is_removed FROM users WHERE is_removed=$1"]
    pub async fn select_many_by_is_removed<'e, E>(
        executor: E,
        is_removed: bool,
    ) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            User,
            "SELECT id, email, is_removed FROM users WHERE is_removed=$1",
            is_removed
        )
        .fetch_all(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

## Insert

### Insert without returning

```rust
use sql_macros::SqlInsert;

#[derive(Debug, SqlInsert)]
#[table(name = "users")]
pub struct CreateUser {
    pub email: String,
}

pub async fn create(pool: &sqlx::PgPool, data: &CreateUser) -> Result<u64, sqlx::Error> {
    let query_result = data.insert(pool).await?;
    Ok(query_result.rows_affected())
}
```

<details>
    <summary>View generated code</summary>

```rust
impl CreateUser {
    #[doc = "INSERT INTO users (email) VALUES ($1)"]
    pub async fn insert<'e, E>(
        &self,
        executor: E,
    ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let query_result = sqlx::query!("INSERT INTO users (email) VALUES ($1)", &self.email)
            .execute(executor)
            .await?;
        Ok(query_result.into())
    }
}
```

</details>

### Insert with returning

```rust
use sql_macros::SqlInsert;

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = User)]
pub struct CreateUser {
    pub email: String,
}

pub async fn create(pool: &sqlx::PgPool, data: &CreateUser) -> Result<User, sqlx::Error> {
    let user = data.insert(pool).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl CreateUser {
    #[doc = "INSERT INTO users (email) VALUES ($1) RETURNING *"]
    pub async fn insert<'e, E>(&self, executor: E) -> Result<User, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            User,
            "INSERT INTO users (email) VALUES ($1) RETURNING *",
            &self.email
        )
        .fetch_one(executor)
        .await?;
        Ok(object)
    }
}

```

</details>

### Insert with returning fields

```rust
use sql_macros::SqlInsert;

#[derive(sqlx::FromRow)]
struct CreateUserResponse {
    pub id: i32,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = CreateUserResponse, return_fields = "id")]
pub struct CreateUser {
    pub email: String,
}

pub async fn create(pool: &sqlx::PgPool, data: &CreateUser) -> Result<CreateUserResponse, sqlx::Error> {
    let user = data.insert(pool).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl CreateUser {
    #[doc = "INSERT INTO users (email) VALUES ($1) RETURNING id"]
    pub async fn insert<'e, E>(&self, executor: E) -> Result<CreateUserResponse, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            CreateUserResponse,
            "INSERT INTO users (email) VALUES ($1) RETURNING id",
            &self.email
        )
        .fetch_one(executor)
        .await?;
        Ok(object)
    }
}

```

</details>

### Insert many

`SqlInsert` only inserts a single row. For inserting a batch of rows in one
round-trip, derive `SqlInsertMany` as well. Because the number of rows isn't
known at compile time, this can't use `sqlx::query!`/`query_as!` like the
other methods do — it's built at runtime with `sqlx::QueryBuilder` instead.

```rust
use sql_macros::SqlInsertMany;

#[derive(Debug, SqlInsertMany)]
#[table(name = "users", return_type = User)]
pub struct CreateUser {
    pub email: String,
}

pub async fn create_many(pool: &sqlx::PgPool, data: &[CreateUser]) -> Result<Vec<User>, sqlx::Error> {
    let users = CreateUser::insert_many(data, pool).await?;
    Ok(users)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl CreateUser {
    #[doc = "INSERT INTO users (email) VALUES (...), (...), ... RETURNING *"]
    pub async fn insert_many<'e, E>(
        items: &[Self],
        executor: E,
    ) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let mut builder = sqlx::QueryBuilder::new("INSERT INTO users (email) ");
        builder.push_values(items, |mut b, item| {
            b.push_bind(&item.email);
        });
        builder.push(" RETURNING ").push("*");

        let objects = builder
            .build_query_as::<User>()
            .fetch_all(executor)
            .await?;
        Ok(objects)
    }
}
```

</details>

Without `return_type`, `insert_many` returns `Result<sqlx::any::AnyQueryResult, sqlx::Error>` instead, same as `insert`.

### Upsert

Mark a field `#[table(upsert)]` to generate its own `upsert_by_<field>`
method on `SqlInsert` — same shape as `select_by_<field>`/
`update_by_<field>`/`delete_by_<field>`: that field becomes the
`ON CONFLICT (...)` target, every other field becomes a `DO UPDATE SET col
= EXCLUDED.col` assignment (or `DO NOTHING` if it's the only field, since an
empty `SET` is invalid SQL). The plain `insert()` method is unaffected —
always a plain `INSERT`, never `ON CONFLICT`, whether or not any field is
marked.

```rust
use sql_macros::SqlInsert;

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = User)]
pub struct UpsertUser {
    #[table(upsert)]
    pub email: String,
    pub is_active: bool,
}

pub async fn upsert(pool: &sqlx::PgPool, data: &UpsertUser) -> Result<User, sqlx::Error> {
    let user = data.upsert_by_email(pool).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl UpsertUser {
    #[doc = "INSERT INTO users (email, is_active) VALUES ($1,$2)"]
    pub async fn insert<'e, E>(&self, executor: E) -> Result<User, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        // ... no `ON CONFLICT` ...
    }

    #[doc = "INSERT INTO users (email, is_active) VALUES ($1,$2) ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active RETURNING *"]
    pub async fn upsert_by_email<'e, E>(&self, executor: E) -> Result<User, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            User,
            "INSERT INTO users (email, is_active) VALUES ($1,$2) ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active RETURNING *",
            &self.email as _,
            &self.is_active as _
        )
        .fetch_one(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

`SqlInsertMany` supports the same `#[table(upsert)]` attribute, naming its
method `upsert_many_by_<field>` instead (not `upsert_by_<field>`, so
deriving both `SqlInsert` and `SqlInsertMany` on one struct doesn't collide)
— the `ON CONFLICT` clause is appended once, after the batch `VALUES (...),
(...), ...` list, and applies to every row.

Marking two (or more) fields bare `#[table(upsert)]` is **not** an error —
each marked field is independent and gets its own method
(`upsert_by_org_id`, `upsert_by_user_id`, ...), never silently combined with
another marked field — same rule as two `#[table(select)]` fields never
combining into one `select_by_...`. For a single `ON CONFLICT` target
spanning more than one column, use a struct-level `#[table(upsert =
method_name(field1, field2, ...))]` attribute instead. It generates an
*additional*, separately named method — the plain `insert()` (or
`insert_many()`) is still generated alongside it, since that method is
never affected by `#[table(upsert)]` in any form. If the struct derives
both `SqlInsert` and `SqlInsertMany`, `SqlInsertMany`'s copy of the method
is named `method_name_many` (not bare `method_name`), same `insert`/
`insert_many` split as everywhere else — so the two don't collide:

```rust
use sql_macros::SqlInsert;

#[derive(Debug, SqlInsert)]
#[table(name = "user_todos", upsert = mark_done(user_id, todo_id))]
pub struct MarkTodoDone {
    pub user_id: i32,
    pub todo_id: i32,
    pub done: bool,
}
```

<details>
    <summary>View generated code</summary>

```rust
impl MarkTodoDone {
    #[doc = "INSERT INTO user_todos (user_id, todo_id, done) VALUES ($1,$2,$3)"]
    pub async fn insert<'e, E>(&self, executor: E) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        // ...
    }

    #[doc = "INSERT INTO user_todos (user_id, todo_id, done) VALUES ($1,$2,$3) ON CONFLICT (user_id, todo_id) DO UPDATE SET done=EXCLUDED.done"]
    pub async fn mark_done<'e, E>(&self, executor: E) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        // ...
    }
}
```

</details>

## Update

### Update without returning

It just return query result (see `sqlx::any::AnyQueryResult`)

```rust
use sql_macros::SqlUpdate;

#[derive(SqlUpdate)]
#[table(name = "users")]
pub struct UpdateUser {
    #[table(update)]
    pub id: i32,
    pub email: String,
}

pub async fn update(pool: &sqlx::PgPool, data: &UpdateUser) -> Result<u64, sqlx::Error> {
    let query_result = data.update_by_id(pool).await?;
    Ok(query_result.rows_affected())
}
```

<details>
    <summary>View generated code</summary>

```rust
impl UpdateUser {
    #[doc = "UPDATE users SET email=$1 WHERE id=$2"]
    pub async fn update_by_id<'e, E>(
        &self,
        executor: E,
    ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let result = sqlx::query!(
            "UPDATE users SET email=$1 WHERE id=$2",
           &self.email,
           &self.id
       )
       .execute(executor)
       .await?;
       Ok(result.into())
   }
}
```

</details>

### Update with returning type

```rust
use sql_macros::SqlUpdate;

#[derive(SqlUpdate)]
#[table(name = "users", return_type = User)]
pub struct UpdateUser {
    #[table(update)]
    pub id: i32,
    pub email: String,
}

pub async fn update(pool: &sqlx::PgPool, data: &UpdateUser) -> Result<User, sqlx::Error> {
    let user = data.update_by_id(pool).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl UpdateUser {
   #[doc = "UPDATE users SET email=$1 WHERE id=$2 RETURNING *"]
   pub async fn update_by_id<'e, E>(&self, executor: E) -> Result<User, sqlx::Error>
   where
       E: sqlx::PgExecutor<'e>,
   {
       let object = sqlx::query_as!(
           User,
           "UPDATE users SET email=$1 WHERE id=$2 RETURNING *",
            &self.email,
            &self.id
        )
        .fetch_one(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

### Update with returning fields

```rust
use sql_macros::SqlUpdate;

#[derive(sqlx::FromRow)]
struct UpdateUserResponse {
    pub id: i32,
}

#[derive(SqlUpdate)]
#[table(name = "users", return_type = UpdateUserResponse, return_fields = "id")]
pub struct UpdateUser {
    #[table(update)]
    pub id: i32,
    pub email: String,
}

pub async fn update(pool: &sqlx::PgPool, data: &UpdateUser) -> Result<UpdateUserResponse, sqlx::Error> {
    let res = data.update_by_id(pool).await?;
    Ok(res)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl UpdateUser {
   #[doc = "UPDATE users SET email=$1 WHERE id=$2 RETURNING id"]
   pub async fn update_by_id<'e, E>(&self, executor: E) -> Result<UpdateUserResponse, sqlx::Error>
   where
       E: sqlx::PgExecutor<'e>,
   {
       let object = sqlx::query_as!(
           UpdateUserResponse,
           "UPDATE users SET email=$1 WHERE id=$2 RETURNING id",
            &self.email,
            &self.id
        )
        .fetch_one(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

### Update with special columns

```rust
use sql_macros::SqlUpdate;

#[derive(SqlUpdate)]
#[table(name = "users", spec_columns = "updated_at=NOW()")]
pub struct UpdateUser {
    #[table(update)]
    pub id: i32,
    pub email: String,
}

pub async fn update(pool: &sqlx::PgPool, data: &UpdateUser) -> Result<u64, sqlx::Error> {
    let query_result = data.update_by_id(pool).await?;
    Ok(query_result.rows_affected())
}
```

<details>
  <summary>View generated code</summary>

```rust
impl UpdateUser {
   #[doc = "UPDATE users SET email=$1, updated_at=NOW() WHERE id=$2"]
   pub async fn update_by_id<'e, E>(
       &self,
       executor: E,
   ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
   where
       E: sqlx::PgExecutor<'e>,
   {
       let result = sqlx::query!(
            "UPDATE users SET email=$1, updated_at=NOW() WHERE id=$2",
            &self.email,
            &self.id
        )
        .execute(executor)
        .await?;
        Ok(result.into())
    }
}
```

</details>

## Delete

```rust
use sql_macros::SqlDelete;

#[derive(SqlDelete)]
pub struct User {
    #[table(delete)]
    pub id: i32,
}

async fn delete(pool: &sqlx::PgPool, id: i32) -> Result<u64, sqlx::Error> {
    let result = User::delete_by_id(pool, id).await?;
    Ok(result.rows_affected())
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "DELETE FROM users WHERE id=$1"]
    pub async fn delete_by_id<'e, E>(
        executor: E,
        id: i32,
    ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let result = sqlx::query!("DELETE FROM users WHERE id=$1", id)
            .execute(executor)
            .await?;
        Ok(result.into())
    }
}
```

</details>

## Generate methods with many fields

```rust
use sql_macros::{SqlDelete, SqlSelect, SqlSelectMany, SqlUpdate};

#[derive(SqlSelect, SqlSelectMany, SqlDelete, SqlUpdate)]
#[table(select = get_active_user(is_active, is_removed))]
#[table(select_many = get_user_by_removed(is_active, is_removed))]
#[table(delete = delete_user(is_active, is_removed))]
#[table(update = update_by_email(email))]
pub struct User {
    pub id: i32,
    pub email: String,
    pub is_active: bool,
    pub is_removed: bool,
}
```

Works with select, select_many, delete, update.

For `update`, the listed fields (`email` above) become the `WHERE` filter
and every other field (`id`, `is_active`, `is_removed`) becomes a `SET`
column - same rule `update_by_<field>` follows via `#[table(update)]` on a
field, just parameterised so you can generate several differently-filtered
update methods on one struct:

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "UPDATE users SET id=$1, is_active=$2, is_removed=$3 WHERE email=$4"]
    pub async fn update_by_email<'e, E>(&self, executor: E) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let result = sqlx::query!(
            "UPDATE users SET id=$1, is_active=$2, is_removed=$3 WHERE email=$4",
            &self.id,
            &self.is_active,
            &self.is_removed,
            &self.email
        )
        .execute(executor)
        .await?;
        Ok(result.into())
    }
}
```

</details>

## Comparison operators

A field's own single-field methods (`select_by_<field>`,
`select_many_by_<field>`, `delete_by_<field>`, `update_by_<field>`) default
to `=`. Add `op = "..."` to use something else:
`gt`/`lt`/`gte`/`lte` (`>`, `<`, `>=`, `<=`), `like`/`ilike` (`LIKE`/`ILIKE`
— `ilike` is Postgres's case-insensitive `LIKE`), or `in`/`not_in`
(`= ANY($n)`/`!= ALL($n)`, which also widen the generated parameter from
`FieldType` to `&[FieldType]`).

**This only applies to a field's own single-field methods** — a custom
multi-field method (`select = name(...)`) never reads it; those are always
`=`, `AND`-joined (see [Custom multi-field methods](#custom-multi-field-methods)
below for how to express something more complex there).

```rust
use sql_macros::SqlSelectMany;

#[derive(SqlSelectMany)]
pub struct User {
    pub email: String,
    #[table(select_many, op = "gt")]
    pub id: i32,
}

pub async fn get_newer_than(pool: &sqlx::PgPool, id: i32) -> Result<Vec<User>, sqlx::Error> {
    let users = User::select_many_by_id_gt(pool, id).await?;
    Ok(users)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT email, id FROM users WHERE id>$1"]
    pub async fn select_many_by_id_gt<'e, E>(executor: E, id: i32) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT email, id FROM users WHERE id>$1", id)
            .fetch_all(executor)
            .await?;
        Ok(object)
    }
}
```

</details>

Note the method name: any non-`eq` operator is suffixed onto the base name
(`select_many_by_id` -> `select_many_by_id_gt`), so the operator is visible
at the call site instead of being hidden behind a plain-looking name.

`op` also accepts a list — `op = ["gt", "lt"]` — generating one method
variant per operator, instead of one field forcing a single fixed
comparison everywhere a field can only have one meaning at a time:

```rust
#[derive(SqlSelectMany)]
pub struct User {
    pub email: String,
    #[table(select_many, op = ["gt", "lt"])]
    pub id: i32,
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT email, id FROM users WHERE id>$1"]
    pub async fn select_many_by_id_gt<'e, E>(executor: E, id: i32) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT email, id FROM users WHERE id>$1", id)
            .fetch_all(executor)
            .await?;
        Ok(object)
    }

    #[doc = "SELECT email, id FROM users WHERE id<$1"]
    pub async fn select_many_by_id_lt<'e, E>(executor: E, id: i32) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT email, id FROM users WHERE id<$1", id)
            .fetch_all(executor)
            .await?;
        Ok(object)
    }
}
```

</details>

This works the same way for `#[table(update)]` fields — `op = ["gt", "lt"]`
generates `update_by_age_gt` and `update_by_age_lt`, same shape as
`select_many_by_age_gt`/`select_many_by_age_lt`.

`op = "in"`/`"not_in"` change the parameter type, since they filter against
a list rather than a single value:

```rust
#[derive(SqlSelectMany)]
pub struct User {
    pub email: String,
    #[table(select_many, op = "in")]
    pub id: i32,
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT email, id FROM users WHERE id = ANY($1)"]
    pub async fn select_many_by_id_in<'e, E>(executor: E, id: &[i32]) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(User, "SELECT email, id FROM users WHERE id = ANY($1)", id)
            .fetch_all(executor)
            .await?;
        Ok(object)
    }
}
```

</details>

## Custom multi-field methods

A custom multi-field method (`select = name(...)`, `select_many = name(...)`,
`update = name(...)`, `delete = name(...)`) is always `=`, `AND`-joined —
`#[table(op = "...")]` never applies here, on purpose: a field's own
comparison operator staying fixed to that field keeps a struct's attributes
readable at a glance, instead of the same field silently meaning something
different in every method that references it.

If you need something more than a plain `AND` of equalities — `OR`, `NOT`,
a different operator, anything — give the method a raw filter template
instead of a field list: a single quoted string where `$field` is *only*
the bound-value placeholder (numbered by first occurrence; the same field
referenced twice reuses one number). The column name, `=`, `OR`, `AND`,
`NOT`, parens, anything else in the string is your own SQL, passed through
unchanged.

```rust
use sql_macros::SqlSelectMany;

#[derive(SqlSelectMany)]
#[table(select_many = search_users(
    "email=$email OR (is_active=$is_active AND NOT is_removed=$is_removed)"
))]
pub struct User {
    pub id: i32,
    pub email: String,
    pub is_active: bool,
    pub is_removed: bool,
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT id, email, is_active, is_removed FROM users WHERE email=$1 OR (is_active=$2 AND NOT is_removed=$3)"]
    pub async fn search_users<'e, E>(
        executor: E,
        email: String,
        is_active: bool,
        is_removed: bool,
    ) -> Result<Vec<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            User,
            "SELECT id, email, is_active, is_removed FROM users WHERE email=$1 OR (is_active=$2 AND NOT is_removed=$3)",
            email,
            is_active,
            is_removed
        )
        .fetch_all(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

This works the same way for `update = name("...")`, except the placeholders
are numbered *after* the `SET` columns, same as a plain field list would be.

## Select with enum

```rust
use sql_macros::SqlSelect;

#[derive(Debug, sqlx::Type)]
#[sqlx(type_name = "role", rename_all = "snake_case")]
pub enum Role {
    Admin,
    User,
    SuperAdmin,
}

#[derive(SqlSelect)]
pub struct User {
    #[table(select)]
    pub id: i32,
    pub email: String,
    #[table(as_type = "role!: Role")]
    pub role: Role,
}

pub async fn get_by_id(pool: &sqlx::PgPool, id: i32) -> Result<Option<User>, sqlx::Error> {
    let user = User::select_by_id(pool, id).await?;
    Ok(user)
}
```

<details>
    <summary>View generated code</summary>

```rust
impl User {
    #[doc = "SELECT id, email, role AS \"role!: Role\" FROM users WHERE id=$1"]
    pub async fn select_by_id<'e, E>(executor: E, id: i32) -> Result<Option<User>, sqlx::Error>
    where
        E: sqlx::PgExecutor<'e>,
    {
        let object = sqlx::query_as!(
            User,
            "SELECT id, email, role AS \"role!: Role\" FROM users WHERE id=$1",
            id
        )
        .fetch_optional(executor)
        .await?;
        Ok(object)
    }
}
```

</details>

## Attention

`return_type` without `return_fields` generates `RETURNING *`. That's a
problem if the return type has a column with a Postgres enum (or other
custom) type — `sqlx::query_as!` needs an `AS "col!: Type"` annotation to
decode it, and `*` can't carry one, since the macro only ever sees
`return_type` as a type name (a token), not the return type's own field
list — it has no way to know that field needs an override.

```rust
#[derive(Debug, sqlx::FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
    pub role: Role,
}

#[derive(Debug, SqlInsert)]
#[table(name = "users", return_type = User)]
pub struct CreateUser {
    pub email: String,
}
```

<details>
    <summary>Why this fails</summary>

```text
error: no built in mapping found for type role of column "role";
a type override may be required, see documentation for details
```

</details>

The fix: write the annotation yourself in `return_fields` — it's inserted
into the query as plain text, so anything valid after `RETURNING` works,
including a type override:

```rust
#[table(name = "users", return_type = User, return_fields = "id, email, role AS \"role!: Role\"")]
```
