use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

fn construction(fields: &Fields, head: TokenStream) -> TokenStream {
    match fields {
        Fields::Named(fields) => {
            let names = fields.named.iter().map(|field| &field.ident);
            quote! { #head { #(#names: decoder.decode()?),* } }
        }
        Fields::Unnamed(fields) => {
            let values = fields.unnamed.iter().map(|_| quote! { decoder.decode()? });
            quote! { #head(#(#values),*) }
        }
        Fields::Unit => head,
    }
}

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let tags = crate::tags::parse(input)?;
    let name = &input.ident;
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter
            .bounds
            .push(syn::parse_quote!(nfs3_types::xdr_codec::Decode));
    }
    let (implementation, types, clause) = generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(data) => {
            let value = construction(&data.fields, quote!(Self));
            quote!(Ok(#value))
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            for (variant, tag) in data.variants.iter().zip(tags) {
                let ident = &variant.ident;
                let value = construction(&variant.fields, quote!(Self::#ident));
                arms.push(quote!(#tag => Ok(#value),));
            }
            quote! {
                match decoder.decode::<u32>()? {
                    #(#arms)*
                    tag => decoder.fail(nfs3_types::xdr_codec::DecodeError::InvalidTag(tag)),
                }
            }
        }
        Data::Union(_) => unreachable!("validated by shared parser"),
    };
    Ok(quote! {
        impl #implementation nfs3_types::xdr_codec::Decode for #name #types #clause {
            fn decode(decoder: &mut nfs3_types::xdr_codec::Decoder)
                -> nfs3_types::xdr_codec::DecodeResult<Self> {
                #body
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_inputs_are_errors() {
        for input in [
            "union Bad { a: u32 }",
            "enum Bad { A(u32) }",
            "enum Bad { #[xdr(no)] A(u32) }",
            "enum Bad { #[xdr(-1)] A(u32) }",
            "enum Bad { #[xdr(FOO)] A(u32) }",
            "enum Bad { #[xdr(4294967296)] A(u32) }",
            "enum Bad { #[xdr(1)] A(u32), #[xdr(1)] B }",
            "enum Bad { #[xdr(1)] #[xdr(2)] A }",
            "enum Bad { #[xdr(0)] A { x: u32 } }",
            "enum Bad { A = 4294967296 }",
            "enum Bad { A = -1 }",
            "enum Bad { A = FOO }",
            "enum Bad { #[xdr(7)] A }",
            "enum Bad { A = 1, B = 1 }",
            "#[xdr(no)] struct Bad;",
            "struct Bad { #[xdr(1)] field: u32 }",
            "enum Bad { #[xdr(1)] A(#[xdr(2)] u32) }",
        ] {
            let input = syn::parse_str(input).expect("valid Rust syntax");
            assert!(expand(&input).is_err());
            assert!(crate::encode::expand(&input).is_err());
        }
    }
}
