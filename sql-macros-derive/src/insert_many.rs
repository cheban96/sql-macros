use quote::quote;

use crate::model::{self, TableModel};

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let table_name = &model.table_name;
    let column_names = model.column_names();

    let all: Vec<&model::Column> = model.columns.iter().collect();
    let idents = model::idents(&all);

    // `sqlx::query!`/`query_as!` need a *static* SQL string with a fixed
    // number of placeholders, so they can't express an INSERT with a
    // variable number of rows. `sqlx::QueryBuilder` builds the
    // `VALUES (...), (...), ...` list at runtime instead, binding each
    // row's columns positionally — the standard way to batch-insert with
    // sqlx.
    let insert_prefix = format!("INSERT INTO {table_name} ({column_names}) ");
    let returning = model.return_fields.as_deref().unwrap_or("*");

    let targets: Vec<&model::Column> = all
        .iter()
        .filter(|c| c.is_upsert_target)
        .copied()
        .collect();
    let upsert_clause = model::upsert_clause(&all, &targets);
    let upsert_doc = upsert_clause.as_deref().unwrap_or("");
    let push_upsert = upsert_clause
        .as_ref()
        .map(|clause| quote! { builder.push(#clause); });

    let tokens = if let Some(return_type) = &model.return_type {
        let doc = format!(
            "INSERT INTO {table_name} ({column_names}) VALUES (...), (...), ...{upsert_doc} RETURNING {returning}"
        );
        quote! {
            impl #struct_name {
                #[doc = #doc]
                pub async fn insert_many(
                    items: &[Self],
                    conn: &mut sqlx::PgConnection,
                ) -> Result<Vec<#return_type>, sqlx::Error> {
                    if items.is_empty() {
                        return Ok(Vec::new());
                    }

                    let mut builder = sqlx::QueryBuilder::new(#insert_prefix);
                    builder.push_values(items, |mut b, item| {
                        #(b.push_bind(&item.#idents);)*
                    });
                    #push_upsert
                    builder.push(" RETURNING ").push(#returning);

                    let objects = builder
                        .build_query_as::<#return_type>()
                        .fetch_all(&mut *conn)
                        .await?;
                    Ok(objects)
                }
            }
        }
    } else {
        let doc =
            format!("INSERT INTO {table_name} ({column_names}) VALUES (...), (...), ...{upsert_doc}");
        quote! {
            impl #struct_name {
                #[doc = #doc]
                pub async fn insert_many(
                    items: &[Self],
                    conn: &mut sqlx::PgConnection,
                ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error> {
                    if items.is_empty() {
                        return Ok(Default::default());
                    }

                    let mut builder = sqlx::QueryBuilder::new(#insert_prefix);
                    builder.push_values(items, |mut b, item| {
                        #(b.push_bind(&item.#idents);)*
                    });
                    #push_upsert

                    let result = builder.build().execute(&mut *conn).await?;
                    Ok(result.into())
                }
            }
        }
    };

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote as q;

    fn parse(tokens: proc_macro2::TokenStream) -> syn::DeriveInput {
        syn::parse2(tokens).expect("failed to parse test struct")
    }

    #[test]
    fn plain_insert_many_has_no_on_conflict_clause() {
        let input = parse(q! {
            struct CreateUser { email: String }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(!tokens.contains("ON CONFLICT"));
    }

    #[test]
    fn upsert_target_generates_on_conflict_do_update() {
        let input = parse(q! {
            struct CreateUser {
                #[table(upsert)]
                email: String,
                is_active: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active"));
    }
}
