use proc_macro::TokenStream;

mod attrs;
mod delete;
mod insert;
mod insert_many;
mod model;
mod select;
mod select_all;
mod select_many;
mod table;
mod update;

use model::TableModel;

/// Parses the derive input, builds the shared `TableModel`, runs `codegen`,
/// and turns any error (attribute parsing or codegen validation) into a
/// span-accurate `compile_error!` instead of panicking.
fn run(
    input: TokenStream,
    codegen: impl FnOnce(&TableModel) -> darling::Result<proc_macro2::TokenStream>,
) -> TokenStream {
    let derive_input = syn::parse_macro_input!(input as syn::DeriveInput);
    let result = TableModel::from_derive_input(&derive_input).and_then(|model| codegen(&model));
    match result {
        Ok(tokens) => tokens.into(),
        Err(err) => err.write_errors().into(),
    }
}

#[proc_macro_derive(SqlSelect, attributes(table))]
pub fn sql_select_macro_derive(input: TokenStream) -> TokenStream {
    run(input, select::expand)
}

#[proc_macro_derive(SqlSelectAll, attributes(table))]
pub fn sql_select_all_macro_derive(input: TokenStream) -> TokenStream {
    run(input, select_all::expand)
}

#[proc_macro_derive(SqlSelectMany, attributes(table))]
pub fn sql_select_many_macro_derive(input: TokenStream) -> TokenStream {
    run(input, select_many::expand)
}

#[proc_macro_derive(SqlInsert, attributes(table))]
pub fn sql_insert_macro_derive(input: TokenStream) -> TokenStream {
    run(input, insert::expand)
}

#[proc_macro_derive(SqlInsertMany, attributes(table))]
pub fn sql_insert_many_macro_derive(input: TokenStream) -> TokenStream {
    run(input, insert_many::expand)
}

#[proc_macro_derive(SqlUpdate, attributes(table))]
pub fn sql_update_macro_derive(input: TokenStream) -> TokenStream {
    run(input, update::expand)
}

#[proc_macro_derive(SqlDelete, attributes(table))]
pub fn sql_delete_macro_derive(input: TokenStream) -> TokenStream {
    run(input, delete::expand)
}

#[proc_macro_derive(SqlTable, attributes(table))]
pub fn sql_table_macro_derive(input: TokenStream) -> TokenStream {
    run(input, table::expand)
}
