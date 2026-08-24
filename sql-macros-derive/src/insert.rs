//! `SqlInsert`: a single-row `insert(&self, conn)` method. Fields marked
//! `#[table(upsert)]` become the `ON CONFLICT (...)` target — every other
//! field becomes a `DO UPDATE SET col = EXCLUDED.col` assignment (or
//! `DO NOTHING` if every field is part of the conflict target).

use quote::quote;

use crate::model::{self, TableModel};

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let table_name = &model.table_name;
    let column_names = model.column_names();

    let all: Vec<&model::Column> = model.columns.iter().collect();
    let idents = model::idents(&all);
    let placeholders = (1..=idents.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(",");

    let targets: Vec<&model::Column> = all
        .iter()
        .filter(|c| c.is_upsert_target)
        .copied()
        .collect();

    let query = format!("INSERT INTO {table_name} ({column_names}) VALUES ({placeholders})");
    let query = match model::upsert_clause(&all, &targets) {
        Some(clause) => format!("{query}{clause}"),
        None => query,
    };
    let returning = model.return_fields.as_deref().unwrap_or("*");
    let bind_args = model::self_bind_tokens(&all);

    let tokens = if let Some(return_type) = &model.return_type {
        let query = format!("{query} RETURNING {returning}");
        quote! {
            impl #struct_name {
                #[doc = #query]
                pub async fn insert(&self, conn: &mut sqlx::PgConnection) -> Result<#return_type, sqlx::Error>
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
        }
    } else {
        quote! {
            impl #struct_name {
                #[doc = #query]
                pub async fn insert(&self, conn: &mut sqlx::PgConnection) -> Result<sqlx::any::AnyQueryResult, sqlx::Error>
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
}
