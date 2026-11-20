use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Index};

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let tags = crate::tags::parse(input)?;
    let name = &input.ident;
    let mut generics = input.generics.clone();
    for parameter in generics.type_params_mut() {
        parameter
            .bounds
            .push(syn::parse_quote!(nfs3_types::xdr_codec::Encode));
    }
    let (implementation, types, clause) = generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(data) => {
            let fields = data.fields.iter().enumerate().map(|(index, field)| {
                let member = field.ident.as_ref().map_or_else(
                    || {
                        let index = Index::from(index);
                        quote!(#index)
                    },
                    |name| quote!(#name),
                );
                quote!(encoder.encode(&self.#member)?;)
            });
            quote! { #(#fields)* Ok(()) }
        }
        Data::Enum(data) => {
            let arms = data.variants.iter().zip(tags).map(|(variant, tag)| {
                let ident = &variant.ident;
                match variant.fields {
                    Fields::Unit => quote!(Self::#ident => encoder.encode(&#tag),),
                    Fields::Unnamed(_) => quote! {
                        Self::#ident(value) => {
                            encoder.encode(&#tag)?;
                            encoder.encode(value)
                        },
                    },
                    Fields::Named(_) => unreachable!("validated by shared parser"),
                }
            });
            quote! { match self { #(#arms)* } }
        }
        Data::Union(_) => unreachable!("validated by shared parser"),
    };
    Ok(quote! {
        impl #implementation nfs3_types::xdr_codec::Encode for #name #types #clause {
            fn encode(&self, encoder: &mut nfs3_types::xdr_codec::Encoder)
                -> nfs3_types::xdr_codec::EncodeResult<()> {
                #body
            }
        }
    })
}
