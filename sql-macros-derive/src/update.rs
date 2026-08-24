use quote::quote;

use crate::model::{self, TableModel};

/// `columns` become the `SET` clause, `filter_choices` become the `WHERE`
/// clause (in that placeholder order, `sql_where` already numbered starting
/// at `columns.len() + 1` — see [`model::sql_condition_for_choices`] /
/// [`model::render_custom_where`]), both read off `self` by field name.
struct GenerateMethodArgs<'a> {
    method_name: &'a syn::Ident,
    table_name: &'a str,
    spec_columns: Option<&'a str>,
    return_type: Option<&'a syn::Type>,
    return_fields: Option<&'a str>,
    columns: &'a [&'a model::Column],
    filter_choices: &'a [model::FilterChoice<'a>],
    sql_where: &'a str,
}

fn generate_method(spec: GenerateMethodArgs) -> darling::Result<proc_macro2::TokenStream> {
    let GenerateMethodArgs {
        method_name,
        table_name,
        spec_columns,
        return_type,
        return_fields,
        columns,
        filter_choices,
        sql_where,
    } = spec;

    if columns.is_empty() {
        return Err(darling::Error::custom(format!(
            "`{method_name}` has no columns left to SET: every field is used as a WHERE filter"
        ))
        .with_span(method_name));
    }

    let filter_columns: Vec<&model::Column> = filter_choices.iter().map(|c| c.column).collect();
    let column_bind_args = model::self_bind_tokens(columns);
    let filter_bind_args = model::self_bind_tokens(&filter_columns);

    let sql_set = model::sql_set_clause(columns, 0);
    let spec_columns_clause = spec_columns.map(|s| format!(", {s}")).unwrap_or_default();

    let query = format!("UPDATE {table_name} SET {sql_set}{spec_columns_clause} WHERE {sql_where}");
    let returning = return_fields.unwrap_or("*");

    let tokens = if let Some(return_type) = return_type {
        let query = format!("{query} RETURNING {returning}");
        quote! {
            #[doc = #query]
            pub async fn #method_name(&self, conn: &mut sqlx::PgConnection) -> Result<#return_type, sqlx::Error> {
                let object = sqlx::query_as!(
                    #return_type,
                    #query,
                    #column_bind_args,
                    #filter_bind_args
                )
                .fetch_one(&mut *conn)
                .await?;
                Ok(object)
            }
        }
    } else {
        quote! {
            #[doc = #query]
            pub async fn #method_name(&self, conn: &mut sqlx::PgConnection) -> Result<sqlx::any::AnyQueryResult, sqlx::Error> {
                let result = sqlx::query!(
                    #query,
                    #column_bind_args,
                    #filter_bind_args
                )
                .execute(&mut *conn)
                .await?;
                Ok(result.into())
            }
        }
    };

    Ok(tokens)
}

pub fn expand(model: &TableModel) -> darling::Result<proc_macro2::TokenStream> {
    let struct_name = &model.struct_name;
    let mut errors = darling::Error::accumulator();
    let mut methods = Vec::new();

    // One method per `#[table(update)]` field (and per operator in its own
    // `op` list) — same shape as `select_by_<field>`/`delete_by_<field>`:
    // that field alone is the WHERE filter, every other field is a SET
    // column. Two different `#[table(update)]` fields never combine into
    // one method, same as two different `#[table(select)]` fields never
    // combine into one `select_by_...`.
    for column in model.columns.iter().filter(|c| c.is_update_filter) {
        for &op in &column.ops {
            let method_name = if op == model::Operator::Eq {
                format!("update_by_{}", column.ident)
            } else {
                format!("update_by_{}_{}", column.ident, model::op_word(op))
            };
            let method_name = syn::Ident::new(&method_name, column.ident.span());

            let set_columns: Vec<&model::Column> = model
                .columns
                .iter()
                .filter(|c| c.ident != column.ident)
                .collect();
            let choices = vec![model::FilterChoice { column, op }];
            let sql_where = model::sql_condition_for_choices(&choices, set_columns.len());

            if let Some(tokens) = errors.handle(generate_method(GenerateMethodArgs {
                method_name: &method_name,
                table_name: &model.table_name,
                spec_columns: model.spec_columns.as_deref(),
                return_type: model.return_type.as_ref(),
                return_fields: model.return_fields.as_deref(),
                columns: &set_columns,
                filter_choices: &choices,
                sql_where: &sql_where,
            })) {
                methods.push(tokens);
            }
        }
    }

    // Custom-named methods, e.g. `#[table(update = update_by_email(email))]`
    // or `#[table(update = restore_by_id("id=$id"))]`: the referenced fields
    // become the WHERE filters for that method (always `=`, never a
    // field's own `op`), every other field is a SET column, same rule as
    // the per-field methods above.
    if let Some(customs) = errors.handle(model.custom_update_methods()) {
        for custom in customs {
            let columns: Vec<&model::Column> = model
                .columns
                .iter()
                .filter(|c| !custom.filters.iter().any(|f| f.ident == c.ident))
                .collect();
            let sql_where = model::render_custom_where(&custom, columns.len());
            let choices: Vec<model::FilterChoice> = custom
                .filters
                .iter()
                .map(|&column| model::FilterChoice {
                    column,
                    op: model::Operator::Eq,
                })
                .collect();

            if let Some(tokens) = errors.handle(generate_method(GenerateMethodArgs {
                method_name: &custom.method_name,
                table_name: &model.table_name,
                spec_columns: model.spec_columns.as_deref(),
                return_type: model.return_type.as_ref(),
                return_fields: model.return_fields.as_deref(),
                columns: &columns,
                filter_choices: &choices,
                sql_where: &sql_where,
            })) {
                methods.push(tokens);
            }
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
    fn custom_update_with_all_fields_as_filters_is_an_error() {
        // Every field is listed as a WHERE filter, so there is nothing
        // left to SET — this used to be silently accepted and would have
        // produced `UPDATE users SET  WHERE ...` (invalid SQL).
        let input = parse(q! {
            #[table(update = touch(id))]
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let err = expand(&model).unwrap_err();
        assert!(err.to_string().contains("no columns left to SET"));
    }

    #[test]
    fn set_clause_with_multiple_columns_is_comma_joined_not_and_joined() {
        // Regression: SET must be `a=$1, b=$2`, not `a=$1 AND b=$2` (the
        // latter is invalid Postgres syntax for an UPDATE's SET list, even
        // though it's exactly the right syntax for a WHERE clause).
        let input = parse(q! {
            struct User {
                #[table(update)]
                id: i32,
                email: String,
                is_active: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("SET email=$1, is_active=$2"));
    }

    #[test]
    fn only_as_type_columns_bind_with_a_cast() {
        // Regression: `as _` used to be applied to every bound field
        // unconditionally, which quietly disabled `sqlx::query!`'s normal
        // compile-time type check for plain columns.
        let input = parse(q! {
            struct User {
                #[table(update)]
                id: i32,
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
        assert!(!tokens.contains("self . id as _"));
    }

    #[test]
    fn raw_filter_where_placeholders_are_numbered_after_the_set_columns() {
        // `SqlUpdate` numbers WHERE placeholders after the SET columns
        // (offset = columns.len()) — a raw-filter custom method has to be
        // shifted the same way a plain field-list filter already is.
        let input = parse(q! {
            #[table(update = restore_by_id_if_removed("id=$id AND is_removed=$is_removed"))]
            struct RestoreUser {
                #[table(update)]
                id: i32,
                email: String,
                is_removed: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains(
            "UPDATE restore_users SET email=$1 WHERE id=$2 AND is_removed=$3"
        ));
    }

    #[test]
    fn no_update_filter_marked_generates_no_default_method() {
        // No `#[table(update)]` anywhere means there's no field to loop
        // over — same as `SqlSelect`/`SqlDelete` generating nothing when no
        // field is marked `#[table(select)]`/`#[table(delete)]`.
        let input = parse(q! {
            struct User { id: i32, email: String }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(!tokens.contains("fn update"));
    }

    #[test]
    fn two_update_filter_fields_generate_two_independent_methods() {
        // Same shape as select_by_<field>/delete_by_<field>: two filter
        // fields never combine into one method.
        let input = parse(q! {
            struct User {
                #[table(update)]
                id: i32,
                #[table(update)]
                email: String,
                is_active: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn update_by_id"));
        assert!(tokens.contains("SET email=$1, is_active=$2 WHERE id=$3"));
        assert!(tokens.contains("fn update_by_email"));
        assert!(tokens.contains("SET id=$1, is_active=$2 WHERE email=$3"));
    }

    #[test]
    fn op_list_on_an_update_filter_generates_one_method_per_operator() {
        let input = parse(q! {
            struct User {
                #[table(update, op = ["gt", "lt"])]
                age: i32,
                email: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn update_by_age_gt"));
        assert!(tokens.contains("WHERE age>$2"));
        assert!(tokens.contains("fn update_by_age_lt"));
        assert!(tokens.contains("WHERE age<$2"));
    }

    #[test]
    fn custom_update_method_field_list_ignores_a_fields_own_op() {
        // Custom methods are always `=`, even if the referenced field also
        // carries `#[table(op = "gt")]` for its own single-field method.
        let input = parse(q! {
            #[table(update = touch_age(age))]
            struct User {
                #[table(update, op = "gt")]
                age: i32,
                email: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let tokens = expand(&model).unwrap().to_string();
        assert!(tokens.contains("fn touch_age"));
        assert!(tokens.contains("WHERE age=$2"));
    }
}
