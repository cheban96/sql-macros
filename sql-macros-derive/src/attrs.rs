//! Attribute parsing for `#[table(...)]`, built on `darling`.
//!
//! This replaces the old hand-rolled `TokenTree` walker. `darling` gives us:
//! - span-accurate `syn::Error`s instead of `panic!` (via `write_errors()`)
//! - automatic support for repeated `#[table(...)]` attributes on one item
//!   (`#[darling(multiple)]`), which is how one struct can define several
//!   `select = name(fields)` style custom methods
//! - a typed field list (`darling::ast::Data`) instead of re-walking
//!   `syn::Fields` by hand in every derive
//!
//! # Breaking change vs. 0.1
//! `#[table(name = users)]` (bare identifier) is no longer accepted; use
//! `#[table(name = "users")]`. Everything else (`return_type = User`,
//! `select = method(fields)`, `as_type = "..."`, `spec_columns = "..."`,
//! `return_fields = "..."`) is unchanged.

use darling::{Error, FromDeriveInput, FromField, FromMeta, ast, util};

/// The field-list side of a custom method spec — either a plain
/// comma-separated list (always `=`, `AND`-joined — `#[table(op = ...)]`
/// doesn't apply to custom methods, only to a field's own single-field
/// methods), or a raw filter template given as a single quoted string,
/// where `$field` is *only* the bound-value placeholder (numbered by first
/// occurrence) and everything else (the column name, `=`, `OR`, `AND`,
/// `NOT`, parens, ...) is the user's own SQL, passed through unchanged. See
/// [`MethodSpec`].
#[derive(Debug, Clone)]
pub enum MethodFilterSpec {
    Fields(Vec<syn::Ident>),
    Raw(String),
}

/// `method_name(field1, field2, ...)` or `method_name("$field OR ...")` —
/// the value side of `#[table(select = get_active_user(is_active,
/// is_removed))]` or `#[table(select = search_users("$email OR $phone"))]`.
///
/// The field-list form can't be a plain string because the field names need
/// to resolve to real struct fields later (and get real spans for error
/// messages), so it's parsed as a call-shaped expression; the raw-template
/// form is a single string-literal argument instead — the only way `AND`,
/// `OR`, `NOT`, etc. can appear at all, since those aren't valid Rust
/// expression tokens.
#[derive(Debug, Clone)]
pub struct MethodSpec {
    pub method_name: syn::Ident,
    pub filter: MethodFilterSpec,
}

impl FromMeta for MethodSpec {
    fn from_expr(expr: &syn::Expr) -> darling::Result<Self> {
        let syn::Expr::Call(call) = expr else {
            return Err(
                Error::custom("expected `method_name(field1, field2, ...)`").with_span(expr)
            );
        };

        let syn::Expr::Path(func_path) = call.func.as_ref() else {
            return Err(Error::custom("expected a method name here").with_span(&call.func));
        };
        let method_name = func_path
            .path
            .get_ident()
            .ok_or_else(|| {
                Error::custom("method name must be a plain identifier").with_span(func_path)
            })?
            .clone();

        // A single quoted-string argument is a raw filter template
        // (`name("$email OR $phone")`) rather than a field list.
        if let [syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        })] = call.args.iter().collect::<Vec<_>>().as_slice()
        {
            return Ok(MethodSpec {
                method_name,
                filter: MethodFilterSpec::Raw(s.value()),
            });
        }

        let fields = call
            .args
            .iter()
            .map(|arg| {
                let syn::Expr::Path(field_path) = arg else {
                    return Err(Error::custom("expected a field name").with_span(arg));
                };
                field_path.path.get_ident().cloned().ok_or_else(|| {
                    Error::custom("field name must be a plain identifier").with_span(field_path)
                })
            })
            .collect::<darling::Result<Vec<_>>>()?;

        if fields.is_empty() {
            return Err(
                Error::custom("expected at least one field name in parentheses").with_span(call),
            );
        }

        Ok(MethodSpec {
            method_name,
            filter: MethodFilterSpec::Fields(fields),
        })
    }
}

/// A string that must be given as a quoted literal (`name = "users"`), with a
/// friendly, span-accurate error if someone writes a bare identifier instead
/// (the most common mistake coming from the old 0.1 syntax).
#[derive(Debug, Clone)]
pub struct QuotedString(pub String);

impl FromMeta for QuotedString {
    fn from_expr(expr: &syn::Expr) -> darling::Result<Self> {
        match expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Ok(QuotedString(s.value())),
            syn::Expr::Path(p) if p.path.get_ident().is_some() => Err(Error::custom(format!(
                "expected a quoted string; write `\"{}\"` instead of `{}`",
                p.path.get_ident().unwrap(),
                p.path.get_ident().unwrap()
            ))
            .with_span(expr)),
            _ => Err(Error::custom("expected a quoted string, e.g. `\"users\"`").with_span(expr)),
        }
    }
}

/// A type given unquoted, e.g. `return_type = User` or `return_type = crate::User`.
/// Parsed from the expression's tokens since attribute values are always
/// `syn::Expr`, never `syn::Type`, at the meta-parsing layer.
#[derive(Debug, Clone)]
pub struct TypeExpr(pub syn::Type);

impl FromMeta for TypeExpr {
    fn from_expr(expr: &syn::Expr) -> darling::Result<Self> {
        let tokens = quote::ToTokens::to_token_stream(expr);
        syn::parse2::<syn::Type>(tokens)
            .map(TypeExpr)
            .map_err(|e| Error::custom(format!("expected a type here: {e}")).with_span(expr))
    }
}

/// The comparison operator for a filter field (`select`, `select_many`,
/// `update`, or `delete`): `#[table(select_many, op = "gt")]`. Defaults to
/// `Eq` when `op` is absent, which is the only operator that existed before
/// this attribute — every existing `#[table(select)]`-style field keeps
/// generating an `=` comparison unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Gt,
    Lt,
    Gte,
    Lte,
    Like,
    /// Postgres-specific case-insensitive `LIKE`.
    Ilike,
    /// `field = ANY($n)`; the generated method takes `&[FieldType]` instead
    /// of `FieldType` for this filter.
    In,
    /// `field != ALL($n)` — the negation of `In`; also takes `&[FieldType]`.
    NotIn,
}

impl Operator {
    pub fn sql_symbol(self) -> &'static str {
        match self {
            Operator::Eq => "=",
            Operator::Gt => ">",
            Operator::Lt => "<",
            Operator::Gte => ">=",
            Operator::Lte => "<=",
            Operator::Like => "LIKE",
            Operator::Ilike => "ILIKE",
            Operator::In => unreachable!("`In` is rendered as `= ANY($n)`, not `<ident> <symbol>`"),
            Operator::NotIn => {
                unreachable!("`NotIn` is rendered as `!= ALL($n)`, not `<ident> <symbol>`")
            }
        }
    }
}

impl FromMeta for Operator {
    fn from_expr(expr: &syn::Expr) -> darling::Result<Self> {
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) = expr
        else {
            return Err(Error::custom("expected a quoted operator, e.g. `op = \"gt\"`")
                .with_span(expr));
        };
        match s.value().as_str() {
            "eq" => Ok(Operator::Eq),
            "gt" => Ok(Operator::Gt),
            "lt" => Ok(Operator::Lt),
            "gte" => Ok(Operator::Gte),
            "lte" => Ok(Operator::Lte),
            "like" => Ok(Operator::Like),
            "ilike" => Ok(Operator::Ilike),
            "in" => Ok(Operator::In),
            "not_in" => Ok(Operator::NotIn),
            other => Err(Error::custom(format!(
                "unknown operator `{other}`; expected one of: eq, gt, lt, gte, lte, like, ilike, in, not_in"
            ))
            .with_span(expr)),
        }
    }
}

/// `op = "gt"` (one operator) or `op = ["gt", "lt"]` (several) — each
/// operator in the list generates its own method variant for that field
/// (`select_by_age` for the implicit `eq`, `select_by_age_gt`,
/// `select_by_age_lt`, ...). Absent, this defaults to a single `Eq`, same
/// as before this attribute existed.
#[derive(Debug, Clone)]
pub struct OperatorList(pub Vec<Operator>);

impl FromMeta for OperatorList {
    fn from_expr(expr: &syn::Expr) -> darling::Result<Self> {
        match expr {
            syn::Expr::Array(array) => {
                let ops = array
                    .elems
                    .iter()
                    .map(Operator::from_expr)
                    .collect::<darling::Result<Vec<_>>>()?;
                if ops.is_empty() {
                    return Err(
                        Error::custom("expected at least one operator in the list")
                            .with_span(array),
                    );
                }
                Ok(OperatorList(ops))
            }
            other => Operator::from_expr(other).map(|op| OperatorList(vec![op])),
        }
    }
}

/// Field-level `#[table(...)]`.
///
/// A field can be a plain column, or opt in to being a filter for `select`,
/// `select_many`, `update`, or `delete` methods; a filter field can also
/// carry an `as_type` override for its `SELECT` column (used for Postgres
/// enums: `role AS "role!: Role"`).
#[derive(Debug, Clone, FromField)]
#[darling(attributes(table))]
pub struct FieldAttrs {
    pub ident: Option<syn::Ident>,
    pub ty: syn::Type,

    #[darling(default)]
    pub select: util::Flag,
    #[darling(default)]
    pub select_many: util::Flag,
    #[darling(default)]
    pub update: util::Flag,
    #[darling(default)]
    pub delete: util::Flag,
    /// Marks this field as part of the `ON CONFLICT (...)` target for
    /// `SqlInsert`/`SqlInsertMany`. Every other field becomes a
    /// `DO UPDATE SET col = EXCLUDED.col` assignment.
    #[darling(default)]
    pub upsert: util::Flag,

    #[darling(default)]
    pub as_type: Option<String>,
    #[darling(default)]
    pub op: Option<OperatorList>,
}

/// Struct-level `#[table(...)]`.
#[derive(Debug, FromDeriveInput)]
#[darling(attributes(table), supports(struct_named))]
pub struct TableAttrs {
    pub ident: syn::Ident,
    pub data: ast::Data<util::Ignored, FieldAttrs>,

    #[darling(default)]
    pub name: Option<QuotedString>,
    #[darling(default)]
    pub spec_columns: Option<QuotedString>,
    #[darling(default)]
    pub return_type: Option<TypeExpr>,
    #[darling(default)]
    pub return_fields: Option<QuotedString>,

    #[darling(default, multiple, rename = "select")]
    pub custom_select: Vec<MethodSpec>,
    #[darling(default, multiple, rename = "select_many")]
    pub custom_select_many: Vec<MethodSpec>,
    #[darling(default, multiple, rename = "delete")]
    pub custom_delete: Vec<MethodSpec>,
    #[darling(default, multiple, rename = "update")]
    pub custom_update: Vec<MethodSpec>,
}
