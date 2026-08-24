use quote::quote;

use crate::model::TableModel;

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let table_name = &model.table_name;
    let sql_columns = model.sql_columns();
    let query = format!("SELECT {sql_columns} FROM {table_name}");

    Ok(quote! {
        impl #struct_name {
            #[doc = #query]
            pub async fn select_all(pool: &sqlx::PgPool) -> Result<Vec<#struct_name>, sqlx::Error> {
                let object = sqlx::query_as!(#struct_name, #query)
                    .fetch_all(pool)
                    .await?;
                Ok(object)
            }
        }
    })
}
