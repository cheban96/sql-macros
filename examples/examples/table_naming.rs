//! Table name resolution and `SqlTable`.
//!
//! With no `#[table(name = "...")]`, the table name is derived from the
//! struct name: split PascalCase into words, lowercase, join with `_`, then
//! append `s` (`UserTodo` -> `user_todo` -> `user_todos`). Use
//! `#[table(name = "...")]` to override it, e.g. when the struct name
//! doesn't match the table name (`CreateUser` -> `users`, used throughout
//! the other examples).
//!
//! `SqlTable` doesn't generate queries; it exposes the resolved table name
//! and column list as `const fn`-free associated functions, for code that
//! wants that metadata without hand-writing it.

use sql_macros::{SqlSelect, SqlTable};

#[derive(Debug, SqlTable, SqlSelect)]
pub struct UserTodo {
    #[table(select)]
    pub id: i32,
    pub user_id: i32,
    pub todo_id: i32,
}

fn main() {
    assert_eq!(UserTodo::name(), "user_todos");
    assert_eq!(UserTodo::fields(), vec!["id", "user_id", "todo_id"]);
    println!("table: {}", UserTodo::name());
    println!("columns: {:?}", UserTodo::sql_columns());
}
