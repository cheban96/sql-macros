//! Shared intermediate representation.
//!
//! Every `#[derive(Sql...)]` macro builds one `TableModel` from the
//! `syn::DeriveInput` and then only reads from it — none of the
//! `select.rs`/`update.rs`/etc. codegen modules touch `syn::Attribute` or
//! `darling` directly. This is the single place that turns "what the user
//! wrote in attributes" into "what SQL/Rust to generate", so the five
//! `generate_method`-style functions that used to duplicate this logic
//! (one per derive) now share one implementation.

use quote::ToTokens;

use crate::attrs::{FieldAttrs, MethodFilterSpec, MethodSpec, TableAttrs};
pub use crate::attrs::Operator;

/// One field of the annotated struct, after attribute parsing.
#[derive(Debug)]
pub struct Column {
    pub ident: syn::Ident,
    pub ty: syn::Type,
    /// The `SELECT`-list entry for this column: `email`, or
    /// `role AS "role!: Role"` when `as_type` is set.
    pub sql_select_expr: String,
    pub is_select_filter: bool,
    pub is_select_many_filter: bool,
    pub is_update_filter: bool,
    pub is_delete_filter: bool,
    pub is_upsert_target: bool,
    /// Set when the field carries `#[table(as_type = "...")]` — a Postgres
    /// enum/domain column whose bind expression needs an explicit `as _` in
    /// generated `INSERT`/`UPDATE` calls (see [`bind_expr`]). `None` for a
    /// plain column, which binds as a bare `self.field` so `sqlx::query!`'s
    /// normal compile-time type check still applies to it.
    pub has_as_type: bool,
    /// `#[table(op = "...")]` or `op = ["...", ...]` — the comparison
    /// operator(s) this column uses in its own single-field methods
    /// (`select_by_<field>`, `select_many_by_<field>`, `delete_by_<field>`,
    /// `update_by_<field>`). Defaults to `[Eq]`. Each operator in the list
    /// generates its own method variant, one field at a time — every
    /// single-field-method derive loops `for op in column.ops`
    /// independently, the same shape for all four. Custom multi-field
    /// methods (`select = name(...)`) never read this; they're always a
    /// plain `=`, `AND`-joined (or, for a raw template, whatever the user
    /// wrote).
    pub ops: Vec<Operator>,
}

impl From<FieldAttrs> for Column {
    fn from(f: FieldAttrs) -> Self {
        let ident = f.ident.expect("named struct fields always have an ident");
        let sql_select_expr = match &f.as_type {
            Some(as_type) => format!("{ident} AS \"{as_type}\""),
            None => ident.to_string(),
        };
        let has_as_type = f.as_type.is_some();
        Column {
            ident,
            ty: f.ty,
            sql_select_expr,
            is_select_filter: f.select.is_present(),
            is_select_many_filter: f.select_many.is_present(),
            is_update_filter: f.update.is_present(),
            is_delete_filter: f.delete.is_present(),
            is_upsert_target: f.upsert.is_present(),
            has_as_type,
            ops: f.op.map(|list| list.0).unwrap_or_else(|| vec![Operator::Eq]),
        }
    }
}

/// One concrete `(column, operator)` assignment for a single generated
/// method variant. A field with `op = ["gt", "lt"]` produces one
/// `FilterChoice` per operator, each its own method; a plain field or a
/// custom method's field list is just `Operator::Eq`.
#[derive(Debug, Clone, Copy)]
pub struct FilterChoice<'a> {
    pub column: &'a Column,
    pub op: Operator,
}

/// The word used in a generated method name for a non-`Eq` operator, e.g.
/// `select_by_age` + `_` + `op_word(Gt)` -> `select_by_age_gt`.
pub fn op_word(op: Operator) -> &'static str {
    match op {
        Operator::Eq => "eq",
        Operator::Gt => "gt",
        Operator::Lt => "lt",
        Operator::Gte => "gte",
        Operator::Lte => "lte",
        Operator::Like => "like",
        Operator::Ilike => "ilike",
        Operator::In => "in",
        Operator::NotIn => "not_in",
    }
}

/// One piece of a raw filter template after `$field` references have been
/// resolved against the struct's columns: either literal text (copied
/// through unchanged) or a reference to a column, whose bound-value
/// placeholder number is only decided at render time (see
/// [`render_custom_where`]), since it depends on the offset the caller needs
/// (e.g. `SqlUpdate` numbers its `WHERE` placeholders after its `SET` ones).
#[derive(Debug)]
pub enum RawSegment<'a> {
    Text(String),
    Field(&'a Column),
}

/// How a [`CustomMethod`]'s `WHERE` text should be built. Custom methods
/// never use a column's `#[table(op = ...)]` — they're always `=`, whether
/// listed as fields (`AND`-joined) or referenced by `$field` in a raw
/// template (just the bound-value placeholder); anything more specific has
/// to be written out by hand in a raw template's own SQL text.
#[derive(Debug)]
pub enum FilterTemplate<'a> {
    /// Plain field list — always `=`, `AND`-joined via [`sql_condition`].
    Fields,
    /// A raw `$field`-templated string the user wrote themselves. `$field`
    /// is only the bound-value placeholder; every character that isn't part
    /// of one passes through unchanged (so `OR`/`AND`/`NOT`/`>`/`LIKE`/
    /// parens/anything else — including the column name — is up to them).
    Raw(Vec<RawSegment<'a>>),
}

/// A named custom method (`select = get_active_user(is_active, is_removed)`
/// or `select = search_users("$email OR $phone")`) resolved against the
/// struct's actual fields, so codegen never has to re-check "does this
/// field exist" itself. `filters` lists the referenced columns in the order
/// their placeholders are numbered (first occurrence, for a raw template);
/// build the actual `WHERE` text with [`render_custom_where`], not
/// `sql_condition` directly, since only that knows how to handle `template`.
#[derive(Debug)]
pub struct CustomMethod<'a> {
    pub method_name: syn::Ident,
    pub filters: Vec<&'a Column>,
    pub template: FilterTemplate<'a>,
}

#[derive(Debug)]
pub struct TableModel {
    pub struct_name: syn::Ident,
    pub table_name: String,
    pub columns: Vec<Column>,
    pub spec_columns: Option<String>,
    pub return_type: Option<syn::Type>,
    pub return_fields: Option<String>,

    custom_select: Vec<MethodSpec>,
    custom_select_many: Vec<MethodSpec>,
    custom_delete: Vec<MethodSpec>,
    custom_update: Vec<MethodSpec>,
}

impl TableModel {
    pub fn from_derive_input(input: &syn::DeriveInput) -> darling::Result<Self> {
        let attrs = <TableAttrs as darling::FromDeriveInput>::from_derive_input(input)?;

        let struct_name = attrs.ident;
        let table_name = attrs
            .name
            .map(|q| q.0)
            .unwrap_or_else(|| format!("{}s", to_snake_case(&struct_name.to_string())));

        let fields = attrs
            .data
            .take_struct()
            .expect("supports(struct_named) guarantees this is a named struct")
            .fields;

        if fields.is_empty() {
            return Err(
                darling::Error::custom("struct must have at least one field")
                    .with_span(&struct_name),
            );
        }

        let columns = fields.into_iter().map(Column::from).collect();

        Ok(TableModel {
            struct_name,
            table_name,
            columns,
            spec_columns: attrs.spec_columns.map(|q| q.0),
            return_type: attrs.return_type.map(|t| t.0),
            return_fields: attrs.return_fields.map(|q| q.0),
            custom_select: attrs.custom_select,
            custom_select_many: attrs.custom_select_many,
            custom_delete: attrs.custom_delete,
            custom_update: attrs.custom_update,
        })
    }

    /// The `SELECT`-list text: plain column names, or `col AS "col!:
    /// Type"` where `as_type` is set. Only valid in a `SELECT` list — an
    /// `INSERT INTO t (...)` column list can't have `AS` in it, so
    /// `SqlInsert`/`SqlInsertMany` use [`column_names`] instead.
    pub fn sql_columns(&self) -> String {
        self.columns
            .iter()
            .map(|c| c.sql_select_expr.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Plain, comma-joined column names — no `as_type` annotation. For an
    /// `INSERT INTO t (...)` column list, where `AS` isn't valid syntax.
    pub fn column_names(&self) -> String {
        self.columns
            .iter()
            .map(|c| c.ident.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn find_column(&self, name: &str, span: &syn::Ident) -> darling::Result<&Column> {
        self.columns.iter().find(|c| c.ident == name).ok_or_else(|| {
            darling::Error::custom(format!("`{name}` is not a field of `{}`", self.struct_name))
                .with_span(span)
        })
    }

    fn resolve(&self, spec: &MethodSpec) -> darling::Result<CustomMethod<'_>> {
        match &spec.filter {
            MethodFilterSpec::Fields(field_idents) => {
                let mut filters = Vec::with_capacity(field_idents.len());
                for field_ident in field_idents {
                    filters.push(self.find_column(&field_ident.to_string(), field_ident)?);
                }
                Ok(CustomMethod {
                    method_name: spec.method_name.clone(),
                    filters,
                    template: FilterTemplate::Fields,
                })
            }
            MethodFilterSpec::Raw(raw) => {
                let (filters, segments) = self.parse_raw_template(&spec.method_name, raw)?;
                Ok(CustomMethod {
                    method_name: spec.method_name.clone(),
                    filters,
                    template: FilterTemplate::Raw(segments),
                })
            }
        }
    }

    /// Splits a raw filter template into literal-text and `$field`
    /// segments, resolving each `$field` against the struct's columns.
    /// `$` is only treated as a field reference when followed by a letter
    /// or `_` (so a literal `$1`-style placeholder, if one ever ended up in
    /// the template by mistake, is left alone rather than misread as a
    /// field named `1`).
    fn parse_raw_template<'a>(
        &'a self,
        method_name: &syn::Ident,
        template: &str,
    ) -> darling::Result<(Vec<&'a Column>, Vec<RawSegment<'a>>)> {
        let chars: Vec<char> = template.chars().collect();
        let mut filters: Vec<&Column> = Vec::new();
        let mut segments: Vec<RawSegment<'a>> = Vec::new();
        let mut text = String::new();
        let mut i = 0;

        while i < chars.len() {
            let starts_field = chars[i] == '$'
                && chars.get(i + 1).is_some_and(|c| c.is_alphabetic() || *c == '_');
            if starts_field {
                let start = i + 1;
                let mut end = start;
                while chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_') {
                    end += 1;
                }
                let name: String = chars[start..end].iter().collect();
                let column = self.find_column(&name, method_name)?;

                if !text.is_empty() {
                    segments.push(RawSegment::Text(std::mem::take(&mut text)));
                }
                if !filters.iter().any(|c| c.ident == column.ident) {
                    filters.push(column);
                }
                segments.push(RawSegment::Field(column));
                i = end;
            } else {
                text.push(chars[i]);
                i += 1;
            }
        }
        if !text.is_empty() {
            segments.push(RawSegment::Text(text));
        }

        Ok((filters, segments))
    }

    fn resolve_all(&self, specs: &[MethodSpec]) -> darling::Result<Vec<CustomMethod<'_>>> {
        let mut errors = darling::Error::accumulator();
        let mut methods = Vec::with_capacity(specs.len());
        for spec in specs {
            if let Some(method) = errors.handle(self.resolve(spec)) {
                methods.push(method);
            }
        }
        errors.finish()?;
        Ok(methods)
    }

    pub fn custom_select_methods(&self) -> darling::Result<Vec<CustomMethod<'_>>> {
        self.resolve_all(&self.custom_select)
    }

    pub fn custom_select_many_methods(&self) -> darling::Result<Vec<CustomMethod<'_>>> {
        self.resolve_all(&self.custom_select_many)
    }

    pub fn custom_delete_methods(&self) -> darling::Result<Vec<CustomMethod<'_>>> {
        self.resolve_all(&self.custom_delete)
    }

    pub fn custom_update_methods(&self) -> darling::Result<Vec<CustomMethod<'_>>> {
        self.resolve_all(&self.custom_update)
    }
}

/// `id: i32, is_removed: bool` — a comma-separated parameter list built
/// straight from `syn::Type`s (no string round-trip, so it can't silently
/// drop generic type arguments like the 0.1 parser did), for a resolved
/// [`FilterChoice`] list — a choice with `op` of `In`/`NotIn` takes
/// `&[FieldType]` instead of `FieldType`, since its filter compares against
/// a list (`field = ANY($n)` / `field != ALL($n)`).
pub fn params_tokens_for_choices(choices: &[FilterChoice]) -> proc_macro2::TokenStream {
    let parts = choices.iter().map(|choice| {
        let ident = &choice.column.ident;
        let ty = &choice.column.ty;
        if matches!(choice.op, Operator::In | Operator::NotIn) {
            quote::quote! { #ident: &[#ty] }
        } else {
            quote::quote! { #ident: #ty }
        }
    });
    quote::quote! { #(#parts),* }
}

pub fn idents(columns: &[&Column]) -> Vec<syn::Ident> {
    columns.iter().map(|c| c.ident.clone()).collect()
}

pub fn idents_for_choices(choices: &[FilterChoice]) -> Vec<syn::Ident> {
    choices.iter().map(|c| c.column.ident.clone()).collect()
}

/// `self.field1, self.field2 as _` — the bind-argument list for
/// `sqlx::query!`/`query_as!` in generated `INSERT`/`UPDATE` methods. Only
/// columns with `#[table(as_type = "...")]` get the `as _` cast, since
/// that's the only case that actually needs it (a custom Postgres
/// enum/domain column, where `sqlx::query!` can't otherwise resolve the
/// bind type against the schema); adding it to every field unconditionally
/// would quietly disable the macro's normal compile-time type check for
/// plain columns too.
pub fn self_bind_tokens(columns: &[&Column]) -> proc_macro2::TokenStream {
    let parts = columns.iter().map(|c| {
        let ident = &c.ident;
        if c.has_as_type {
            quote::quote! { self.#ident as _ }
        } else {
            quote::quote! { self.#ident }
        }
    });
    quote::quote! { #(#parts),* }
}

/// `id=$1`, `age>$2`, `email LIKE $3`, `id = ANY($4)` — one column's full
/// comparison at a given placeholder number, for a specific operator.
/// Shared by [`sql_condition_for_choices`] (an `AND`-joined list of these)
/// and single-field method generation in `select.rs`/`select_many.rs`/
/// `delete.rs`/`update.rs`.
fn render_comparison(ident: &syn::Ident, op: Operator, placeholder: usize) -> String {
    match op {
        Operator::In => format!("{ident} = ANY(${placeholder})"),
        Operator::NotIn => format!("{ident} != ALL(${placeholder})"),
        Operator::Like => format!("{ident} LIKE ${placeholder}"),
        Operator::Ilike => format!("{ident} ILIKE ${placeholder}"),
        op => format!("{ident}{}${placeholder}", op.sql_symbol()),
    }
}

/// `id=$1 AND is_removed=$2`, always `=`, placeholders numbered starting at
/// `offset + 1`. Used for a plain field-list custom method — those never
/// read a column's `#[table(op = ...)]`.
pub fn sql_condition(columns: &[&Column], offset: usize) -> String {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| render_comparison(&c.ident, Operator::Eq, offset + i + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Like [`sql_condition`], but for a resolved [`FilterChoice`] list, so each
/// column's comparison can use a specific operator (one variant of its
/// `#[table(op = ...)]` list) instead of always `=`. Used for single-field
/// methods (`select_by_<field>`, ...) and `SqlUpdate`'s default `update()`.
pub fn sql_condition_for_choices(choices: &[FilterChoice], offset: usize) -> String {
    choices
        .iter()
        .enumerate()
        .map(|(i, c)| render_comparison(&c.column.ident, c.op, offset + i + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// Renders a [`CustomMethod`]'s `WHERE` text at the given placeholder
/// `offset` — `sql_condition(&method.filters, offset)` for a plain field
/// list (always `=`), or the user's raw template for a raw one. In a raw
/// template, `$field` is *only* the bound-value placeholder — the column
/// name, operator, and everything else is whatever the user wrote — with
/// the placeholder numbered by first occurrence (`position` from
/// `method.filters`, so the same field referenced twice reuses one number).
pub fn render_custom_where(method: &CustomMethod, offset: usize) -> String {
    match &method.template {
        FilterTemplate::Fields => sql_condition(&method.filters, offset),
        FilterTemplate::Raw(segments) => {
            let mut out = String::new();
            for segment in segments {
                match segment {
                    RawSegment::Text(text) => out.push_str(text),
                    RawSegment::Field(column) => {
                        let position = method
                            .filters
                            .iter()
                            .position(|c| c.ident == column.ident)
                            .expect("every field in a raw template is collected into filters during parse_raw_template");
                        out.push('$');
                        out.push_str(&(offset + position + 1).to_string());
                    }
                }
            }
            out
        }
    }
}

/// `email=$1, is_active=$2` — comma-joined `SET`-clause assignments,
/// placeholders numbered starting at `offset + 1`. Distinct from
/// `sql_condition` (` AND `-joined, for `WHERE`) because joining a `SET`
/// list with `AND` produces a single boolean-valued assignment to the first
/// column instead of one assignment per column.
pub fn sql_set_clause(columns: &[&Column], offset: usize) -> String {
    columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}=${}", c.ident, offset + i + 1))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `ON CONFLICT (email) DO UPDATE SET is_active=EXCLUDED.is_active`, built
/// from the fields marked `#[table(upsert)]` (`targets`, the conflict
/// target) against every other column in `all` (the `DO UPDATE SET` list).
/// `None` if `targets` is empty (no upsert requested). Falls back to
/// `DO NOTHING` if every column is part of the conflict target, since an
/// empty `SET` list is invalid SQL.
pub fn upsert_clause(all: &[&Column], targets: &[&Column]) -> Option<String> {
    if targets.is_empty() {
        return None;
    }

    let target_list = targets
        .iter()
        .map(|c| c.ident.to_string())
        .collect::<Vec<_>>()
        .join(", ");

    let set_columns: Vec<&&Column> = all
        .iter()
        .filter(|c| !targets.iter().any(|t| t.ident == c.ident))
        .collect();

    if set_columns.is_empty() {
        return Some(format!(" ON CONFLICT ({target_list}) DO NOTHING"));
    }

    let set_clause = set_columns
        .iter()
        .map(|c| format!("{ident}=EXCLUDED.{ident}", ident = c.ident))
        .collect::<Vec<_>>()
        .join(", ");

    Some(format!(" ON CONFLICT ({target_list}) DO UPDATE SET {set_clause}"))
}

/// `OrganizationMember` -> `organization_member`. Splits before an uppercase
/// letter that follows a lowercase/digit, or that precedes a lowercase letter
/// after a run of uppercase letters (so `HTTPServer` -> `http_server`).
fn to_snake_case(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + 4);
    for (i, &c) in chars.iter().enumerate() {
        if c.is_uppercase() {
            let prev = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1);
            let boundary = match prev {
                Some(p) => {
                    p.is_lowercase()
                        || p.is_numeric()
                        || (p.is_uppercase() && next.is_some_and(|n| n.is_lowercase()))
                }
                None => false,
            };
            if boundary {
                result.push('_');
            }
            result.extend(c.to_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Renders a `syn::Type` back to source text for embedding in generated
/// code via `syn::parse_str` at a call site that already has a
/// `proc_macro2::TokenStream` slot (kept only where callers need a
/// `&syn::Type`, e.g. `quote! { Result<#ty, ...> }` — most call sites can
/// just interpolate `#ty` directly with `quote!`).
#[allow(dead_code)]
pub fn type_to_string(ty: &syn::Type) -> String {
    ty.to_token_stream().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn parse(tokens: proc_macro2::TokenStream) -> syn::DeriveInput {
        syn::parse2(tokens).expect("failed to parse test struct")
    }

    #[test]
    fn table_name_defaults_to_pluralized_lowercase_struct_name() {
        let input = parse(quote! {
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        assert_eq!(model.table_name, "users");
    }

    #[test]
    fn table_name_requires_quotes_now() {
        // Regression: 0.1 accepted a bare identifier (`name = users`); 0.2
        // requires a quoted string and explains why in the error.
        let input = parse(quote! {
            #[table(name = users)]
            struct User { id: i32 }
        });
        let err = TableModel::from_derive_input(&input).unwrap_err();
        assert!(err.to_string().contains("quoted string"));
    }

    #[test]
    fn table_name_splits_camel_case_struct_name_into_snake_case() {
        let input = parse(quote! {
            struct UserTodo { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        assert_eq!(model.table_name, "user_todos");
    }

    #[test]
    fn table_name_can_be_overridden_with_quotes() {
        let input = parse(quote! {
            #[table(name = "app_users")]
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        assert_eq!(model.table_name, "app_users");
    }

    #[test]
    fn sql_columns_handles_as_type_annotation() {
        let input = parse(quote! {
            struct User {
                id: i32,
                #[table(as_type = "role!: Role")]
                role: Role,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        assert_eq!(model.sql_columns(), "id, role AS \"role!: Role\"");
    }

    #[test]
    fn field_types_with_multiple_generic_args_are_preserved() {
        // Regression test for the 0.1 bug where a hand-rolled string
        // round-trip silently dropped field types with zero or multiple
        // generic arguments (e.g. HashMap<K, V>).
        let input = parse(quote! {
            struct Payload {
                #[table(select)]
                data: std::collections::HashMap<String, i32>,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let ty = &model.columns[0].ty;
        assert_eq!(
            quote!(#ty).to_string(),
            quote!(std::collections::HashMap<String, i32>).to_string()
        );
    }

    #[test]
    fn custom_select_method_resolves_named_fields() {
        let input = parse(quote! {
            #[table(select = get_active_user(is_active, is_removed))]
            struct User {
                id: i32,
                is_active: bool,
                is_removed: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_methods().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].method_name.to_string(), "get_active_user");
        let names: Vec<String> = methods[0]
            .filters
            .iter()
            .map(|c| c.ident.to_string())
            .collect();
        assert_eq!(names, vec!["is_active", "is_removed"]);
    }

    #[test]
    fn custom_method_referencing_unknown_field_is_a_spanned_error() {
        let input = parse(quote! {
            #[table(select = get_by_x(nonexistent))]
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let err = model.custom_select_methods().unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
        assert!(err.to_string().contains("not a field"));
    }

    #[test]
    fn repeated_select_attributes_all_collected() {
        let input = parse(quote! {
            #[table(select = a(id))]
            #[table(select = b(id))]
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_methods().unwrap();
        let names: Vec<String> = methods.iter().map(|m| m.method_name.to_string()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn sql_condition_numbers_placeholders_from_offset() {
        let input = parse(quote! {
            struct User { id: i32, email: String }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let cols: Vec<&Column> = model.columns.iter().collect();
        assert_eq!(sql_condition(&cols, 2), "id=$3 AND email=$4");
    }

    #[test]
    fn custom_method_field_list_ignores_a_fields_own_op() {
        // A custom multi-field method is always `=`, even if the field
        // also carries `#[table(op = "gt")]` for its own single-field
        // methods.
        let input = parse(quote! {
            #[table(select = get_by_age(age))]
            struct User {
                #[table(select_many, op = "gt")]
                age: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_methods().unwrap();
        assert_eq!(render_custom_where(&methods[0], 0), "age=$1");
    }

    #[test]
    fn op_defaults_to_a_single_eq() {
        let input = parse(quote! {
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        assert_eq!(model.columns[0].ops, vec![Operator::Eq]);
    }

    #[test]
    fn op_list_generates_one_choice_per_operator() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = ["gt", "lt"])]
                age: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let column = &model.columns[0];
        assert_eq!(column.ops, vec![Operator::Gt, Operator::Lt]);
        let choices: Vec<String> = column
            .ops
            .iter()
            .map(|&op| sql_condition_for_choices(&[FilterChoice { column, op }], 0))
            .collect();
        assert_eq!(choices, vec!["age>$1", "age<$1"]);
    }

    #[test]
    fn op_ilike_renders_as_case_insensitive_like() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = "ilike")]
                email: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let column = &model.columns[0];
        let choice = FilterChoice { column, op: column.ops[0] };
        assert_eq!(sql_condition_for_choices(&[choice], 0), "email ILIKE $1");
    }

    #[test]
    fn op_in_widens_the_param_type_to_a_slice() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = "in")]
                id: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let column = &model.columns[0];
        let choice = FilterChoice { column, op: column.ops[0] };
        assert_eq!(sql_condition_for_choices(&[choice], 0), "id = ANY($1)");
        assert_eq!(
            params_tokens_for_choices(&[choice]).to_string(),
            quote!(id: &[i32]).to_string()
        );
    }

    #[test]
    fn op_not_in_widens_the_param_type_to_a_slice() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = "not_in")]
                id: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let column = &model.columns[0];
        let choice = FilterChoice { column, op: column.ops[0] };
        assert_eq!(sql_condition_for_choices(&[choice], 0), "id != ALL($1)");
        assert_eq!(
            params_tokens_for_choices(&[choice]).to_string(),
            quote!(id: &[i32]).to_string()
        );
    }

    #[test]
    fn unknown_op_is_a_spanned_error() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = "nope")]
                age: i32,
            }
        });
        let err = TableModel::from_derive_input(&input).unwrap_err();
        assert!(err.to_string().contains("unknown operator"));
    }

    #[test]
    fn empty_op_list_is_a_spanned_error() {
        let input = parse(quote! {
            struct User {
                #[table(select_many, op = [])]
                age: i32,
            }
        });
        let err = TableModel::from_derive_input(&input).unwrap_err();
        assert!(err.to_string().contains("at least one operator"));
    }

    #[test]
    fn raw_filter_template_is_a_bare_placeholder_not_a_whole_comparison() {
        // `$field` is only the bound-value placeholder — the user writes
        // the column name and comparison themselves.
        let input = parse(quote! {
            #[table(select_many = search_users("email=$email OR (phone=$phone AND NOT is_removed=$is_removed)"))]
            struct User {
                id: i32,
                email: String,
                phone: String,
                is_removed: bool,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_many_methods().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(
            render_custom_where(&methods[0], 0),
            "email=$1 OR (phone=$2 AND NOT is_removed=$3)"
        );
    }

    #[test]
    fn raw_filter_template_ignores_a_fields_own_op() {
        let input = parse(quote! {
            #[table(select_many = search_users("email=$email OR age>$age"))]
            struct User {
                id: i32,
                email: String,
                #[table(op = "gt")]
                age: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_many_methods().unwrap();
        assert_eq!(render_custom_where(&methods[0], 0), "email=$1 OR age>$2");
    }

    #[test]
    fn raw_filter_template_collects_referenced_fields_in_first_occurrence_order() {
        let input = parse(quote! {
            #[table(select_many = search_users("$email OR $phone"))]
            struct User {
                id: i32,
                email: String,
                phone: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_many_methods().unwrap();
        let names: Vec<String> = methods[0]
            .filters
            .iter()
            .map(|c| c.ident.to_string())
            .collect();
        assert_eq!(names, vec!["email", "phone"]);
    }

    #[test]
    fn raw_filter_template_reuses_the_same_placeholder_for_a_repeated_field() {
        let input = parse(quote! {
            #[table(select_many = search_users("$age OR $age"))]
            struct User {
                id: i32,
                age: i32,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_many_methods().unwrap();
        // `$age` appears twice but must reuse one placeholder both times.
        assert_eq!(render_custom_where(&methods[0], 0), "$1 OR $1");
        let names: Vec<String> = methods[0]
            .filters
            .iter()
            .map(|c| c.ident.to_string())
            .collect();
        assert_eq!(names, vec!["age"]);
    }

    #[test]
    fn raw_filter_template_offset_shifts_every_placeholder() {
        // SqlUpdate numbers its WHERE placeholders after its SET columns —
        // render_custom_where needs to shift a raw template the same way
        // sql_condition already shifts a plain field list.
        let input = parse(quote! {
            #[table(select_many = search_users("$email OR $phone"))]
            struct User {
                id: i32,
                email: String,
                phone: String,
            }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let methods = model.custom_select_many_methods().unwrap();
        assert_eq!(render_custom_where(&methods[0], 2), "$3 OR $4");
    }

    #[test]
    fn raw_filter_template_unknown_field_is_a_spanned_error() {
        let input = parse(quote! {
            #[table(select_many = search_users("$nonexistent"))]
            struct User { id: i32 }
        });
        let model = TableModel::from_derive_input(&input).unwrap();
        let err = model.custom_select_many_methods().unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
        assert!(err.to_string().contains("not a field"));
    }
}
