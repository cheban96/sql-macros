use quote::quote;

use crate::model::TableModel;

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let table_name = &model.table_name;
    let sql_columns: Vec<&str> = model
        .columns
        .iter()
        .map(|c| c.sql_select_expr.as_str())
        .collect();
    let struct_fields: Vec<String> = model.columns.iter().map(|c| c.ident.to_string()).collect();

    Ok(quote! {
        impl sql_macros::SqlTable for #struct_name {
            fn name() -> &'static str {
                #table_name
            }
            fn fields() -> Vec<&'static str> {
                vec![
                    #(#struct_fields),*
                ]
            }
            fn sql_columns() -> Vec<&'static str> {
                vec![
                    #(#sql_columns),*
                ]
            }
        }
    })
}
