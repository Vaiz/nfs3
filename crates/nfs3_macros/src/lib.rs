#![doc = include_str!("../README.md")]

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, parse_macro_input, Attribute, Meta, Expr, Lit};

/// Helper function to parse #[xdr(value)] attribute
fn parse_xdr_value(attrs: &[Attribute]) -> Option<u32> {
    for attr in attrs {
        if attr.path().is_ident("xdr") {
            if let Meta::List(meta_list) = &attr.meta {
                if let Ok(Expr::Lit(syn::ExprLit { lit: Lit::Int(lit_int), .. })) = meta_list.parse_args::<Expr>() {
                    if let Ok(value) = lit_int.base10_parse::<u32>() {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

#[proc_macro_derive(XdrCodec, attributes(xdr))]
#[allow(clippy::missing_panics_doc, clippy::too_many_lines, clippy::cognitive_complexity)]
pub fn derive_xdr_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = input.generics;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    match &input.data {
        Data::Struct(DataStruct { fields, .. }) => match fields {
            Fields::Named(named_fields) => {
                let pack_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        total_write += self.#name.pack(out)?;
                    }
                });
                let packed_size_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        total_size += nfs3_types::xdr_codec::Pack::packed_size(&self.#name);
                    }
                });
                let unpack_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        let (#name, read_bytes) = nfs3_types::xdr_codec::Unpack::unpack(input)?;
                        total_read += read_bytes;
                    }
                });
                let struct_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! { #name, }
                });
                quote! {
                    impl #impl_generics nfs3_types::xdr_codec::Pack for #name #ty_generics
                    #where_clause {
                        fn packed_size(&self) -> usize {
                            let mut total_size = 0;
                            #(#packed_size_fields)*
                            total_size
                        }

                        fn pack(&self, out: &mut impl std::io::Write) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            let mut total_write = 0;
                            #(#pack_fields)*
                            Ok(total_write)
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::Unpack for #name #ty_generics
                    #where_clause {
                        fn unpack(input: &mut impl std::io::Read) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            use nfs3_types::xdr_codec::Unpack;
                            let mut total_read = 0;
                            #(#unpack_fields)*
                            Ok((Self { #(#struct_fields)* }, total_read))
                        }
                    }
                }
            }
            Fields::Unnamed(unnamed_fields) => {
                let pack_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let index = syn::Index::from(i);
                    quote! {
                        total_write += self.#index.pack(out)?;
                    }
                });
                let packed_size_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let index = syn::Index::from(i);
                    quote! {
                        total_size += nfs3_types::xdr_codec::Pack::packed_size(&self.#index);
                    }
                });
                let unpack_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let var_name = syn::Ident::new(&format!("field_{i}"), proc_macro2::Span::call_site());
                    quote! {
                        let (#var_name, read_bytes) = nfs3_types::xdr_codec::Unpack::unpack(input)?;
                        total_read += read_bytes;
                    }
                });
                let struct_fields = (0..unnamed_fields.unnamed.len()).map(|i| {
                    let var_name = syn::Ident::new(&format!("field_{i}"), proc_macro2::Span::call_site());
                    quote! { #var_name }
                });
                quote! {
                    impl #impl_generics nfs3_types::xdr_codec::Pack for #name #ty_generics
                    #where_clause {
                        fn packed_size(&self) -> usize {
                            let mut total_size = 0;
                            #(#packed_size_fields)*
                            total_size
                        }

                        fn pack(&self, out: &mut impl std::io::Write) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            let mut total_write = 0;
                            #(#pack_fields)*
                            Ok(total_write)
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::Unpack for #name #ty_generics
                    #where_clause {
                        fn unpack(input: &mut impl std::io::Read) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            use nfs3_types::xdr_codec::Unpack;
                            let mut total_read = 0;
                            #(#unpack_fields)*
                            Ok((Self(#(#struct_fields),*), total_read))
                        }
                    }
                }
            }
            Fields::Unit => quote! {
                impl #impl_generics nfs3_types::xdr_codec::Pack for #name #ty_generics
                #where_clause {
                    fn packed_size(&self) -> usize {
                        0
                    }

                    fn pack(&self, _out: &mut impl std::io::Write) -> nfs3_types::xdr_codec::Result<usize> {
                        Ok(0)
                    }
                }
                impl #impl_generics nfs3_types::xdr_codec::Unpack for #name #ty_generics
                #where_clause {
                    fn unpack(_input: &mut impl std::io::Read) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                        Ok((Self, 0))
                    }
                }
            },
        },
        Data::Enum(data) => {
            // Check if this is a simple enum (all unit variants) or complex enum (has data variants)
            let has_data_variants = data.variants.iter().any(|v| !matches!(v.fields, Fields::Unit));
            
            if has_data_variants {
                // Complex enum with data fields - use XDR attributes for discriminant values
                let pack_variants = data.variants.iter().map(|v| {
                    let ident = &v.ident;
                    let xdr_value = parse_xdr_value(&v.attrs).unwrap_or_else(|| {
                        panic!("Complex enum variant '{ident}' must have #[xdr(value)] attribute");
                    });
                    
                    match &v.fields {
                        Fields::Unit => {
                            quote! {
                                Self::#ident => #xdr_value.pack(out),
                            }
                        }
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                            quote! {
                                Self::#ident(val) => {
                                    let mut len = #xdr_value.pack(out)?;
                                    len += val.pack(out)?;
                                    Ok(len)
                                },
                            }
                        }
                        _ => panic!("Complex enum variant '{ident}' must be either unit or have exactly one unnamed field"),
                    }
                });
                
                let packed_size_variants = data.variants.iter().map(|v| {
                    let ident = &v.ident;
                    match &v.fields {
                        Fields::Unit => {
                            quote! {
                                Self::#ident => 4,
                            }
                        }
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                            quote! {
                                Self::#ident(val) => 4 + val.packed_size(),
                            }
                        }
                        _ => panic!("Complex enum variant '{ident}' must be either unit or have exactly one unnamed field"),
                    }
                });
                
                let unpack_variants = data.variants.iter().map(|v| {
                    let ident = &v.ident;
                    let xdr_value = parse_xdr_value(&v.attrs).unwrap_or_else(|| {
                        panic!("Complex enum variant '{ident}' must have #[xdr(value)] attribute");
                    });
                    
                    match &v.fields {
                        Fields::Unit => {
                            quote! {
                                #xdr_value => Ok(Self::#ident),
                            }
                        }
                        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                            quote! {
                                #xdr_value => {
                                    let (val, val_bytes) = nfs3_types::xdr_codec::Unpack::unpack(input)?;
                                    bytes_read += val_bytes;
                                    Ok(Self::#ident(val))
                                },
                            }
                        }
                        _ => panic!("Complex enum variant '{ident}' must be either unit or have exactly one unnamed field"),
                    }
                });

                quote! {
                    impl #impl_generics nfs3_types::xdr_codec::Pack for #name #ty_generics
                    #where_clause {
                        fn packed_size(&self) -> usize {
                            match self {
                                #(#packed_size_variants)*
                            }
                        }

                        fn pack(&self, out: &mut impl std::io::Write) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            match self {
                                #(#pack_variants)*
                            }
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::Unpack for #name #ty_generics
                    #where_clause {
                        fn unpack(input: &mut impl std::io::Read) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            use nfs3_types::xdr_codec::Unpack;
                            let (tag, mut bytes_read) = u32::unpack(input)?;
                            let result = match tag {
                                #(#unpack_variants)*
                                _ => Err(nfs3_types::xdr_codec::Error::InvalidEnumValue(tag)),
                            };
                            result.map(|value| (value, bytes_read))
                        }
                    }
                }
            } else {
                // Simple enum - use existing logic with cast to u32
                let pack_variants = data.variants.iter().map(|v| {
                    let ident = &v.ident;
                    quote! {
                        Self::#ident => (*self as u32).pack(out),
                    }
                });
                let unpack_variants = data.variants.iter().map(|v| {
                    let ident = &v.ident;
                    quote! {
                        x if x == Self::#ident as u32 => Ok(Self::#ident),
                    }
                });

                quote! {
                    impl #impl_generics nfs3_types::xdr_codec::Pack for #name #ty_generics
                    #where_clause {
                        fn packed_size(&self) -> usize {
                            4
                        }

                        fn pack(&self, out: &mut impl std::io::Write) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            match self {
                                #(#pack_variants)*
                            }
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::Unpack for #name #ty_generics
                    #where_clause {
                        fn unpack(input: &mut impl std::io::Read) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            let (tag, bytes_read) = u32::unpack(input)?;
                            let result = match tag {
                                #(#unpack_variants)*
                                _ => Err(nfs3_types::xdr_codec::Error::InvalidEnumValue(tag)),
                            };
                            result.map(|value| (value, bytes_read))
                        }
                    }
                }
            }
        }
        Data::Union(_) => panic!("XdrCodec can only be derived for structs and enums"),
    }
        .into()
}
