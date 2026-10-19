# Roadmap

Candidate features, not yet implemented. Not a commitment or an ordered
priority list — just ideas worth considering next.

1. `update_many` / `delete_many` — batch versions symmetric to `insert_many` (batch currently exists only for insert).
2. Optional/conditional filters — an `Option<T>` field in select/update participates in `WHERE` only when `Some(...)`, without hand-rolling the query.
3. `ORDER BY` / `LIMIT` / `OFFSET` for `select_many` — pagination out of the box.
4. `COUNT`/`EXISTS` — a derive or attribute for `SELECT COUNT(*)` / `SELECT EXISTS(...)`.
5. Composite filters with explicit column name vs. field name — currently Rust field name = column name is hardcoded; `#[table(column = "...")]` would help when they diverge.
6. JSON/JSONB shortcut — something like `#[table(json)]` instead of manually casting via `as_type`.
7. Support for other databases (MySQL/SQLite via `sqlx::Any`) — currently hardcoded to `Postgres`/`PgConnection`.
8. `#[table(default)]` on a field — so `SqlInsert` skips fields with a DB default without requiring them in the struct explicitly (partially already true if the field is simply omitted).
9. Generalize `SqlInsert`/`SqlInsertMany`/`SqlUpdate`/`SqlDelete` from `&mut sqlx::PgConnection` to `impl sqlx::PgExecutor<'_>` too (like `select`/`select_all`/`select_many` now are) — would let a write fire directly against `&pool` without pre-acquiring a connection. Lower priority than the select-side fix since `&mut PgConnection` already accepts `&mut *tx` (a transaction derefs to it), so writes already compose into transactions today; this would just add the `&pool`-direct convenience.

## Done

- Upsert — `ON CONFLICT (...) DO UPDATE` for `SqlInsert`/`SqlInsertMany` via field-level `#[table(upsert)]` (at most one field — two is a compile error), plus a struct-level `#[table(upsert = method_name(field1, field2, ...))]` for a composite conflict target spanning multiple columns, which generates an *additional*, separately named method alongside the always-present `insert()`/`insert_many()`. **Breaking change** from the original design: marking 2+ fields bare `#[table(upsert)]` used to silently combine them into one composite target — that's a compile error now, since "each field responsible for itself, use an explicit function for combining multiple" is the same principle applied everywhere else (`select`/`update`/`delete`).
- `SqlSelect`/`SqlSelectAll`/`SqlSelectMany` take `impl sqlx::PgExecutor<'_>` instead of a fixed `&sqlx::PgPool` — `&pool` still works everywhere (non-breaking), but now so does `&mut *tx` inside a transaction or `&mut conn` for a single connection. Fixes "how do I read inside a transaction?", since a bare `&PgPool` and a `Transaction`/`PgConnection` are unrelated types with no coercion between them.
- `SqlUpdate` generates one `update_by_<field>` method per `#[table(update)]` field, same shape as `select_by_<field>`/`select_many_by_<field>`/`delete_by_<field>` — two different `#[table(update)]` fields never combine into one method (each is its own independent method, filtering on just that field; every other field becomes the `SET` list for that method). **Breaking rename** from the previous single combined `update()` method — every doc example and the `examples/` crate now call `.update_by_id(conn)`, not `.update(conn)`.
- Comparison operators beyond `=` — `#[table(op = "...")]` (`gt`/`lt`/`gte`/`lte`/`like`/`ilike`/`in`/`not_in`), single value or a list (`op = ["gt", "lt"]`, one method variant per operator, e.g. `select_by_age_gt` + `select_by_age_lt`). Applies only to a field's own single-field methods (`select_by_<field>`, `select_many_by_<field>`, `delete_by_<field>`, `update_by_<field>`) — deliberately *not* to custom multi-field methods, so a struct's fields stay readable at a glance instead of hiding a different operator per method.
- `AND`/`OR`/`NOT` (and any other SQL) between filter fields in custom multi-field methods — not via Rust's `&&`/`||`/`!` operators as first proposed, but a raw `"$field"`-templated string: `select_many = search_users("email=$email OR (is_active=$is_active AND NOT is_removed=$is_removed)")`. `$field` is *only* the bound-value placeholder (numbered by first occurrence, reused if a field repeats) — the column name, operator, everything else is the user's own SQL, passed through unchanged; custom methods never read a field's `op`.
