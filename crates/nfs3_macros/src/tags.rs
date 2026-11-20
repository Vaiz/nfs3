use std::collections::BTreeSet;

use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Expr, Fields, Lit, Meta};

fn reject_tags(attrs: &[Attribute]) -> syn::Result<()> {
    if let Some(attr) = attrs.iter().find(|attr| attr.path().is_ident("xdr")) {
        return Err(syn::Error::new(
            attr.span(),
            "xdr tags belong on data-carrying enum variants only",
        ));
    }
    Ok(())
}

fn literal(expr: &Expr) -> syn::Result<u32> {
    if let Expr::Lit(value) = expr {
        if let Lit::Int(value) = &value.lit {
            return value.base10_parse();
        }
    }
    Err(syn::Error::new(expr.span(), "expected a u32 integer tag"))
}

// One validated interpretation shared by the new derives only. Legacy derives
// deliberately keep their original parsing and diagnostics.
pub fn parse(input: &DeriveInput) -> syn::Result<Vec<u32>> {
    reject_tags(&input.attrs)?;
    let data = match &input.data {
        Data::Struct(data) => {
            for field in &data.fields {
                reject_tags(&field.attrs)?;
            }
            return Ok(Vec::new());
        }
        Data::Enum(data) => data,
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "XDR derives support structs and enums only",
            ));
        }
    };
    let tagged = data
        .variants
        .iter()
        .any(|v| !matches!(v.fields, Fields::Unit));
    let mut tags = BTreeSet::new();
    let mut next = Some(0_u32);
    let mut result = Vec::new();
    for variant in &data.variants {
        for field in &variant.fields {
            reject_tags(&field.attrs)?;
        }
        if !tagged {
            reject_tags(&variant.attrs)?;
        }
        let attrs: Vec<_> = variant
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("xdr"))
            .collect();
        if attrs.len() > 1 {
            return Err(syn::Error::new(
                variant.span(),
                "duplicate xdr tag attribute",
            ));
        }
        let tag = if tagged {
            if variant.discriminant.is_some() {
                return Err(syn::Error::new(
                    variant.span(),
                    "use only #[xdr(tag)] in tagged unions",
                ));
            }
            let attr = attrs
                .first()
                .ok_or_else(|| syn::Error::new(variant.span(), "missing #[xdr(tag)]"))?;
            let Meta::List(list) = &attr.meta else {
                return Err(syn::Error::new(attr.span(), "expected #[xdr(tag)]"));
            };
            literal(&list.parse_args::<Expr>()?)?
        } else if let Some((_, expr)) = &variant.discriminant {
            literal(expr)?
        } else {
            next.ok_or_else(|| syn::Error::new(variant.span(), "tag overflow"))?
        };
        next = tag.checked_add(1);
        if !tags.insert(tag) {
            return Err(syn::Error::new(variant.span(), "duplicate XDR tag"));
        }
        if !matches!(&variant.fields, Fields::Unit)
            && !matches!(&variant.fields, Fields::Unnamed(f) if f.unnamed.len() == 1)
        {
            return Err(syn::Error::new(
                variant.span(),
                "union arms must be unit or a single unnamed field",
            ));
        }
        result.push(tag);
    }
    Ok(result)
}
