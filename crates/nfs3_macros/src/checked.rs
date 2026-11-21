use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Index};

#[derive(Clone, Copy)]
pub enum Direction {
    Encode,
    Decode,
}

// Visit each field once for both directions. The entry points select a body
// and its independent bounds, not a second schema/parser implementation.
fn fields(fields: &Fields, head: TokenStream) -> (TokenStream, TokenStream) {
    let mut encodes = Vec::new();
    let mut decodes = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let member = field.ident.as_ref().map_or_else(
            || {
                let index = Index::from(index);
                quote!(#index)
            },
            |name| quote!(#name),
        );
        encodes.push(quote!(encoder.encode(&self.#member)?;));
        decodes.push(field.ident.as_ref().map_or_else(
            || quote!(decoder.decode()?),
            |name| quote!(#name: decoder.decode()?),
        ));
    }
    let value = match fields {
        Fields::Named(_) => quote!(#head { #(#decodes),* }),
        Fields::Unnamed(_) => quote!(#head(#(#decodes),*)),
        Fields::Unit => head,
    };
    (quote!(#(#encodes)* Ok(())), quote!(Ok(#value)))
}

pub fn expand(input: &DeriveInput, direction: Direction) -> syn::Result<TokenStream> {
    let tags = crate::tags::parse(input)?;
    let name = &input.ident;
    let (encode, decode) = match &input.data {
        Data::Struct(data) => fields(&data.fields, quote!(Self)),
        Data::Enum(data) => {
            let mut encodes = Vec::new();
            let mut decodes = Vec::new();
            for (variant, tag) in data.variants.iter().zip(tags) {
                let ident = &variant.ident;
                let (_, decode) = fields(&variant.fields, quote!(Self::#ident));
                let encode = match &variant.fields {
                    Fields::Unit => quote!(Self::#ident => encoder.encode(&#tag),),
                    Fields::Unnamed(_) => quote! {
                        Self::#ident(value) => {
                            encoder.encode(&#tag)?;
                            encoder.encode(value)
                        },
                    },
                    Fields::Named(_) => unreachable!("validated by shared parser"),
                };
                encodes.push(encode);
                decodes.push(quote!(#tag => { #decode },));
            }
            (
                quote!(match self { #(#encodes)* }),
                quote! {
                    match decoder.decode::<u32>()? {
                        #(#decodes)*
                        tag => decoder.fail(nfs3_types::xdr_codec::DecodeError::InvalidTag(tag)),
                    }
                },
            )
        }
        Data::Union(_) => unreachable!("validated by shared parser"),
    };
    let mut generics = input.generics.clone();
    let bound: syn::TypeParamBound = match direction {
        Direction::Encode => syn::parse_quote!(nfs3_types::xdr_codec::Encode),
        Direction::Decode => syn::parse_quote!(nfs3_types::xdr_codec::Decode),
    };
    for parameter in generics.type_params_mut() {
        parameter.bounds.push(bound.clone());
    }
    let (implementation, types, clause) = generics.split_for_impl();
    Ok(match direction {
        Direction::Encode => quote! {
            impl #implementation nfs3_types::xdr_codec::Encode for #name #types #clause {
                fn encode(&self, encoder: &mut nfs3_types::xdr_codec::Encoder)
                    -> nfs3_types::xdr_codec::EncodeResult<()> {
                    #encode
                }
            }
        },
        Direction::Decode => quote! {
            impl #implementation nfs3_types::xdr_codec::Decode for #name #types #clause {
                fn decode(decoder: &mut nfs3_types::xdr_codec::Decoder)
                    -> nfs3_types::xdr_codec::DecodeResult<Self> {
                    #decode
                }
            }
        },
    })
}
