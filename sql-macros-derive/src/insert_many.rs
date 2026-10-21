use quote::quote;

use crate::model::{self, TableModel};

/// Everything about a single `SqlInsertMany` derive that's the same across
/// every generated method (the default `insert_many()` and any
/// composite-upsert methods) — only `method_name` and `targets` vary per
/// method.
struct InsertManyContext<'a> {
    table_name: &'a str,
    column_names: String,
    all: Vec<&'a model::Column>,
    idents: Vec<syn::Ident>,
    return_type: Option<&'a syn::Type>,
    return_fields: Option<&'a str>,
}

fn generate_method(
    ctx: &InsertManyContext<'_>,
    method_name: &syn::Ident,
    targets: &[&model::Column],
) -> proc_macro2::TokenStream {
    let InsertManyContext {
        table_name,
        column_names,
        all,
        idents,
        return_type,
        return_fields,
    } = ctx;
    // `sqlx::query!`/`query_as!` need a *static* SQL string with a fixed
    // number of placeholders, so they can't express an INSERT with a
    // variable number of rows. `sqlx::QueryBuilder` builds the
    // `VALUES (...), (...), ...` list at runtime instead, binding each
    // row's columns positionally — the standard way to batch-insert with
    // sqlx.
    let insert_prefix = format!("INSERT INTO {table_name} ({column_names}) ");
    let returning = return_fields.unwrap_or("*");

    let upsert_clause = model::upsert_clause(all, targets);
    let upsert_doc = upsert_clause.as_deref().unwrap_or("");
    let push_upsert = upsert_clause
        .as_ref()
        .map(|clause| quote! { builder.push(#clause); });

    if let Some(return_type) = return_type {
        let doc = format!(
            "INSERT INTO {table_name} ({column_names}) VALUES (...), (...), ...{upsert_doc} RETURNING {returning}"
        );
        quote! {
            #[doc = #doc]
            pub async fn #method_name<'e, E>(
                items: &[Self],
                executor: E,
            ) -> Result<Vec<#return_type>, sqlx::Error>
            where
                E: sqlx::PgExecutor<'e>,
            {
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
                    .fetch_all(executor)
                    .await?;
                Ok(objects)
            }
        }
    } else {
        let doc = format!(
            "INSERT INTO {table_name} ({column_names}) VALUES (...), (...), ...{upsert_doc}"
        );
        quote! {
            #[doc = #doc]
            pub async fn #method_name<'e, E>(
                items: &[Self],
                executor: E,
            ) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
            where
                E: sqlx::PgExecutor<'e>,
            {
                if items.is_empty() {
                    return Ok(Default::default());
                }

                let mut builder = sqlx::QueryBuilder::new(#insert_prefix);
                builder.push_values(items, |mut b, item| {
                    #(b.push_bind(&item.#idents);)*
                });
                #push_upsert

                let result = builder.build().execute(executor).await?;
                Ok(result.into())
            }
        }
    }
}

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;

    let all: Vec<&model::Column> = model.columns.iter().collect();
    let idents = model::idents(&all);

    let ctx = InsertManyContext {
        table_name: &model.table_name,
        column_names: model.column_names(),
        all,
        idents,
        return_type: model.return_type.as_ref(),
        return_fields: model.return_fields.as_deref(),
    };

    let mut errors = darling::Error::accumulator();
    let mut methods = Vec::new();

    // `insert_many()` — always generated, always plain (no `ON CONFLICT`).
    let default_method_name = syn::Ident::new("insert_many", struct_name.span());
    methods.push(generate_method(&ctx, &default_method_name, &[]));

    // One `upsert_many_by_<field>` method per bare-`#[table(upsert)]` field
    // — `_many_by_` (not `_by_`) so this doesn't collide with `SqlInsert`'s
    // `upsert_by_<field>` when a struct derives both.
    for column in model.upsert_targets() {
        let method_name =
            syn::Ident::new(&format!("upsert_many_by_{}", column.ident), column.ident.span());
        methods.push(generate_method(&ctx, &method_name, &[column]));
    }

    // Struct-level composite upsert, e.g. `#[table(upsert =
    // mark_done(user_id, todo_id))]`. Named `<method_name>_many` (not bare
    // `<method_name>`) — same `insert`/`insert_many` split — so this
    // doesn't collide with `SqlInsert`'s own method of that name when a
    // struct derives both.
    if let Some(customs) = errors.handle(model.custom_upsert_methods()) {
        for custom in customs {
            let method_name =
                syn::Ident::new(&format!("{}_many", custom.method_name), custom.method_name.span());
            methods.push(generate_method(&ctx, &method_name, &custom.targets));
        }
    }

    errors.finish()?;

    Ok(quote! {
        impl #struct_name {
            #(#methods)*
        }
    })
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
    fn bare_upsert_field_generates_its_own_upsert_many_by_method() {
        let input = parse(q! {
            struct CreateUser {
                #[table(upsert)]
                email: String,
                is_active: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn insert_many <"));
        assert!(tokens.contains("fn upsert_many_by_email"));
        assert!(tokens.contains("ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active"));
    }

    #[test]
    fn two_bare_upsert_fields_generate_two_independent_methods() {
        let input = parse(q! {
            struct AddMember {
                #[table(upsert)]
                org_id: i32,
                #[table(upsert)]
                user_id: i32,
                role: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn upsert_many_by_org_id"));
        assert!(tokens.contains("ON CONFLICT (org_id) DO UPDATE SET user_id=EXCLUDED.user_id, role=EXCLUDED.role"));
        assert!(tokens.contains("fn upsert_many_by_user_id"));
        assert!(tokens.contains("ON CONFLICT (user_id) DO UPDATE SET org_id=EXCLUDED.org_id, role=EXCLUDED.role"));
    }

    #[test]
    fn composite_upsert_generates_an_additional_named_method_suffixed_many() {
        // Suffixed `_many` (not the bare `method_name` from the attribute)
        // so this doesn't collide with `SqlInsert`'s own method of that
        // name when a struct derives both.
        let input = parse(q! {
            #[table(upsert = insert_or_update_members(org_id, user_id))]
            struct AddMember {
                org_id: i32,
                user_id: i32,
                role: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn insert_many <"));
        assert!(tokens.contains("fn insert_or_update_members_many <"));
        assert!(tokens.contains("ON CONFLICT (org_id, user_id) DO UPDATE SET role=EXCLUDED.role"));
    }
}
