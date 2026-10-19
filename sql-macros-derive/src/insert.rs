//! `SqlInsert`: a single-row `insert(&self, conn)` method, always generated.
//! A single field marked bare `#[table(upsert)]` makes it the `ON CONFLICT
//! (...)` target for that method — every other field becomes a `DO UPDATE
//! SET col = EXCLUDED.col` assignment (or `DO NOTHING` if every field is
//! part of the conflict target). More than one bare-marked field is a
//! compile error (ambiguous — see `model::TableModel::upsert_target`); for
//! a composite target spanning more than one field, `#[table(upsert =
//! method_name(field1, field2, ...))]` on the struct generates an
//! *additional*, separately named method instead, alongside the always
//!-present `insert()`.

use quote::quote;

use crate::model::{self, TableModel};

/// Everything about a single `SqlInsert` derive that's the same across every
/// generated method (the default `insert()` and any composite-upsert
/// methods) — only `method_name` and `targets` vary per method.
struct InsertContext<'a> {
    table_name: &'a str,
    column_names: String,
    placeholders: String,
    all: Vec<&'a model::Column>,
    return_type: Option<&'a syn::Type>,
    return_fields: Option<&'a str>,
}

fn generate_method(
    ctx: &InsertContext<'_>,
    method_name: &syn::Ident,
    targets: &[&model::Column],
) -> proc_macro2::TokenStream {
    let InsertContext {
        table_name,
        column_names,
        placeholders,
        all,
        return_type,
        return_fields,
    } = ctx;
    let bind_args = model::self_bind_tokens(all);

    let query = format!("INSERT INTO {table_name} ({column_names}) VALUES ({placeholders})");
    let query = match model::upsert_clause(all, targets) {
        Some(clause) => format!("{query}{clause}"),
        None => query,
    };
    let returning = return_fields.unwrap_or("*");

    if let Some(return_type) = return_type {
        let query = format!("{query} RETURNING {returning}");
        quote! {
            #[doc = #query]
            pub async fn #method_name(&self, conn: &mut sqlx::PgConnection) -> Result<#return_type, sqlx::Error>
            {
                let object = sqlx::query_as!(
                    #return_type,
                    #query,
                    #bind_args
                )
                .fetch_one(&mut *conn)
                .await?;
                Ok(object)
            }
        }
    } else {
        quote! {
            #[doc = #query]
            pub async fn #method_name(&self, conn: &mut sqlx::PgConnection) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
            {
                let query_result = sqlx::query!(
                    #query,
                    #bind_args
                )
                .execute(&mut *conn)
                .await?;
                Ok(query_result.into())
            }
        }
    }
}

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;

    let all: Vec<&model::Column> = model.columns.iter().collect();
    let idents = model::idents(&all);
    let placeholders = (1..=idents.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(",");

    let ctx = InsertContext {
        table_name: &model.table_name,
        column_names: model.column_names(),
        placeholders,
        all,
        return_type: model.return_type.as_ref(),
        return_fields: model.return_fields.as_deref(),
    };

    let mut errors = darling::Error::accumulator();
    let mut methods = Vec::new();

    // The default `insert()` — always generated. Its `ON CONFLICT` target
    // is the single bare-`#[table(upsert)]` field, if any (none -> a plain
    // insert, no `ON CONFLICT` at all).
    let default_method_name = syn::Ident::new("insert", struct_name.span());
    if let Some(target) = errors.handle(model.upsert_target()) {
        let targets: Vec<&model::Column> = target.into_iter().collect();
        methods.push(generate_method(&ctx, &default_method_name, &targets));
    }

    // Additional named methods for a composite `ON CONFLICT` target, e.g.
    // `#[table(upsert = insert_or_update_member(org_id, user_id))]`.
    if let Some(customs) = errors.handle(model.custom_upsert_methods()) {
        for custom in customs {
            methods.push(generate_method(&ctx, &custom.method_name, &custom.targets));
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
    fn plain_insert_has_no_on_conflict_clause() {
        let input = parse(q! {
            struct CreateUser { email: String }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(!tokens.contains("ON CONFLICT"));
    }

    #[test]
    fn plain_columns_bind_without_a_cast() {
        let input = parse(q! {
            struct CreateUser { email: String, is_active: bool }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(!tokens.contains("as _"));
    }

    #[test]
    fn as_type_column_binds_with_a_cast_others_do_not() {
        // Regression: `as _` used to be applied to every field unconditionally
        // (originally added only to make enum columns bind correctly), which
        // quietly disabled `sqlx::query!`'s normal compile-time type check
        // for plain columns too. Only the `as_type` column should get it.
        let input = parse(q! {
            struct CreateUser {
                email: String,
                #[table(as_type = "role!: Role")]
                role: Role,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("self . email"));
        assert!(!tokens.contains("self . email as _"));
        assert!(tokens.contains("self . role as _"));
    }

    #[test]
    fn insert_column_list_has_no_as_annotation() {
        // Regression: the INSERT column list used to be built from the
        // SELECT-list text (`sql_columns()`), which appends `AS "col!:
        // Type"` for `as_type` fields — invalid syntax in an `INSERT INTO
        // t (...)` column list.
        let input = parse(q! {
            struct CreateUser {
                #[table(as_type = "role!: Role")]
                role: Role,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("INSERT INTO create_users (role)"));
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

    #[test]
    fn upsert_with_every_field_as_target_does_nothing_on_conflict() {
        let input = parse(q! {
            struct CreateUser {
                #[table(upsert)]
                email: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("ON CONFLICT (email) DO NOTHING"));
    }

    #[test]
    fn two_bare_upsert_fields_is_a_spanned_error() {
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
        let err = expand(&model).unwrap_err();
        assert!(err.to_string().contains("more than one field"));
        assert!(err.to_string().contains("upsert = method_name"));
    }

    #[test]
    fn composite_upsert_generates_an_additional_named_method() {
        let input = parse(q! {
            #[table(upsert = insert_or_update_member(org_id, user_id))]
            struct AddMember {
                org_id: i32,
                user_id: i32,
                role: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        // The plain `insert()` is still generated (no bare-marked field),
        // alongside the composite-conflict named method.
        assert!(tokens.contains("fn insert ("));
        assert!(tokens.contains("fn insert_or_update_member"));
        assert!(tokens.contains("ON CONFLICT (org_id, user_id) DO UPDATE SET role=EXCLUDED.role"));
    }

    #[test]
    fn composite_upsert_with_single_field_is_a_spanned_error() {
        let input = parse(q! {
            #[table(upsert = touch(org_id))]
            struct AddMember {
                org_id: i32,
                role: String,
            }
        });
        let model = TableModel::from_derive_input(&input);
        let err = model.unwrap_err();
        assert!(err.to_string().contains("at least two field names"));
    }
}
