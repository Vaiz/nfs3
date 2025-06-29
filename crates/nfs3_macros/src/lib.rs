#![doc = include_str!("../README.md")]

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, parse_macro_input};

#[proc_macro_derive(XdrCodec)]
#[allow(clippy::missing_panics_doc, clippy::too_many_lines)]
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
        Data::Union(_) => panic!("XdrCodec can only be derived for structs and enums"),
    }
        .into()
}
