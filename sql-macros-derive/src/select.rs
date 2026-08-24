use quote::quote;

use crate::model::{self, TableModel};

fn generate_method(
    method_name: &syn::Ident,
    struct_name: &syn::Ident,
    sql_columns: &str,
    table_name: &str,
    sql_filters: &str,
    choices: &[model::FilterChoice],
) -> proc_macro2::TokenStream {
    let params = model::params_tokens_for_choices(choices);
    let idents = model::idents_for_choices(choices);
    let query = format!("SELECT {sql_columns} FROM {table_name} WHERE {sql_filters}");

    quote! {
        #[doc = #query]
        pub async fn #method_name(pool: &sqlx::PgPool, #params) -> Result<Option<#struct_name>, sqlx::Error> {
            let object = sqlx::query_as!(
                #struct_name,
                #query,
                #(#idents),*
            )
            .fetch_optional(pool)
            .await?;
            Ok(object)
        }
    }
}

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let sql_columns = model.sql_columns();
    let mut methods = Vec::new();

    for column in model.columns.iter().filter(|c| c.is_select_filter) {
        for &op in &column.ops {
            let method_name = if op == model::Operator::Eq {
                format!("select_by_{}", column.ident)
            } else {
                format!("select_by_{}_{}", column.ident, model::op_word(op))
            };
            let method_name = syn::Ident::new(&method_name, column.ident.span());
            let choices = vec![model::FilterChoice { column, op }];
            let sql_filters = model::sql_condition_for_choices(&choices, 0);
            methods.push(generate_method(
                &method_name,
                struct_name,
                &sql_columns,
                &model.table_name,
                &sql_filters,
                &choices,
            ));
        }
    }

    for custom in model.custom_select_methods()? {
        let sql_filters = model::render_custom_where(&custom, 0);
        let choices: Vec<model::FilterChoice> = custom
            .filters
            .iter()
            .map(|&column| model::FilterChoice {
                column,
                op: model::Operator::Eq,
            })
            .collect();
        methods.push(generate_method(
            &custom.method_name,
            struct_name,
            &sql_columns,
            &model.table_name,
            &sql_filters,
            &choices,
        ));
    }

    Ok(quote! {
        impl #struct_name {
            #(#methods)*
        }
    })
}
