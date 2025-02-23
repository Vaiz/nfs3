#![doc = include_str!("../README.md")]

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, parse_macro_input};

#[proc_macro_derive(XdrCodec)]
pub fn derive_xdr_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = input.generics;

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let mut pack_generics = generics.clone();
    pack_generics
        .params
        .push(syn::parse_quote! { _XdrCodecOut: std::io::Write });
    let (pack_impl_generics, _, pack_where_clause) = pack_generics.split_for_impl();

    let mut unpack_generics = generics.clone();
    unpack_generics
        .params
        .push(syn::parse_quote! { _XdrCodecIn: std::io::Read });
    let (unpack_impl_generics, _, unpack_where_clause) = unpack_generics.split_for_impl();

    match &input.data {
        Data::Struct(DataStruct { fields, .. }) => match fields {
            Fields::Named(named_fields) => {
                let serialize_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        total_write += self.#name.pack(dest)?;
                    }
                });
                let count_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        total_write += self.#name.packed_size();
                    }
                });
                let deserialize_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! {
                        let (#name, read_bytes) = Unpack::unpack(src)?;
                        total_read += read_bytes;
                    }
                });
                let struct_fields = named_fields.named.iter().map(|f| {
                    let name = &f.ident;
                    quote! { #name, }
                });
                quote! {
                    impl #pack_impl_generics nfs3_types::xdr_codec::Pack<_XdrCodecOut> for #name #ty_generics
                    #pack_where_clause {
                        fn pack(&self, dest: &mut _XdrCodecOut) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            let mut total_write = 0;
                            #(#serialize_fields)*
                            Ok(total_write)
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::PackedSize for #name #ty_generics
                    #where_clause {
                        const PACKED_SIZE: Option<usize> = None; // TODO: try to evaluate the value

                        fn count_packed_size(&self) -> usize {
                            let mut total_write = 0;
                            #(#count_fields)*
                            total_write
                        }
                    }
                    impl #unpack_impl_generics nfs3_types::xdr_codec::Unpack<_XdrCodecIn> for #name #ty_generics
                    #unpack_where_clause {
                        fn unpack(src: &mut _XdrCodecIn) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            use nfs3_types::xdr_codec::Unpack;
                            let mut total_read = 0;
                            #(#deserialize_fields)*
                            Ok((Self { #(#struct_fields)* }, total_read))
                        }
                    }
                }
            }
            Fields::Unnamed(unnamed_fields) => {
                let serialize_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let index = syn::Index::from(i);
                    quote! {
                        total_write += self.#index.pack(dest)?;
                    }
                });
                let count_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let index = syn::Index::from(i);
                    quote! {
                        total_write += self.#index.packed_size();
                    }
                });
                let deserialize_fields = unnamed_fields.unnamed.iter().enumerate().map(|(i, _)| {
                    let var_name = syn::Ident::new(&format!("field_{}", i), proc_macro2::Span::call_site());
                    quote! {
                        let (#var_name, read_bytes) = Unpack::unpack(src)?;
                        total_read += read_bytes;
                    }
                });
                let struct_fields = (0..unnamed_fields.unnamed.len()).map(|i| {
                    let var_name = syn::Ident::new(&format!("field_{}", i), proc_macro2::Span::call_site());
                    quote! { #var_name }
                });
                quote! {
                    impl #pack_impl_generics nfs3_types::xdr_codec::Pack<_XdrCodecOut> for #name #ty_generics
                    #pack_where_clause {
                        fn pack(&self, dest: &mut _XdrCodecOut) -> nfs3_types::xdr_codec::Result<usize> {
                            use nfs3_types::xdr_codec::Pack;
                            let mut total_write = 0;
                            #(#serialize_fields)*
                            Ok(total_write)
                        }
                    }
                    impl #impl_generics nfs3_types::xdr_codec::PackedSize for #name #ty_generics
                    #where_clause {
                        const PACKED_SIZE: Option<usize> = None; // TODO: try to evaluate the value

                        fn count_packed_size(&self) -> usize {
                            let mut total_write = 0;
                            #(#count_fields)*
                            total_write
                        }
                    }
                    impl #unpack_impl_generics nfs3_types::xdr_codec::Unpack<_XdrCodecIn> for #name #ty_generics
                    #unpack_where_clause {
                        fn unpack(src: &mut _XdrCodecIn) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                            use nfs3_types::xdr_codec::Unpack;
                            let mut total_read = 0;
                            #(#deserialize_fields)*
                            Ok((Self(#(#struct_fields),*), total_read))
                        }
                    }
                }
            }
            Fields::Unit => quote! {
                impl #pack_impl_generics nfs3_types::xdr_codec::Pack<_XdrCodecOut> for #name #ty_generics
                #pack_where_clause {
                    fn pack(&self, _dest: &mut _XdrCodecOut) -> nfs3_types::xdr_codec::Result<usize> {
                        Ok(0)
                    }
                }
                impl #impl_generics nfs3_types::xdr_codec::PackedSize for #name #ty_generics
                #where_clause {
                    const PACKED_SIZE: Option<usize> = Some(0);

                    fn count_packed_size(&self) -> usize {
                        0
                    }
                }
                impl #unpack_impl_generics nfs3_types::xdr_codec::Unpack<_XdrCodecIn> for #name #ty_generics
                #unpack_where_clause {
                    fn unpack(_src: &mut _XdrCodecIn) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                        Ok((Self, 0))
                    }
                }
            },
        },
        Data::Enum(data) => {
            let deserialize_variants = data.variants.iter().map(|v| {
                let ident = &v.ident;
                quote! {
                    x if x == Self::#ident as u32 => Ok(Self::#ident),
                }
            });

            quote! {
                impl #pack_impl_generics nfs3_types::xdr_codec::Pack<_XdrCodecOut> for #name #ty_generics
                #pack_where_clause {
                    fn pack(&self, dest: &mut _XdrCodecOut) -> nfs3_types::xdr_codec::Result<usize> {
                        use byteorder::WriteBytesExt;
                        dest.write_u32::<byteorder::BigEndian>(*self as u32)?;
                        Ok(4)
                    }
                }
                impl #impl_generics nfs3_types::xdr_codec::PackedSize for #name #ty_generics
                #where_clause {
                    const PACKED_SIZE: Option<usize> = Some(4);

                    fn count_packed_size(&self) -> usize {
                        4
                    }
                }
                impl #unpack_impl_generics nfs3_types::xdr_codec::Unpack<_XdrCodecIn> for #name #ty_generics
                #unpack_where_clause {
                    fn unpack(src: &mut _XdrCodecIn) -> nfs3_types::xdr_codec::Result<(Self, usize)> {
                        use byteorder::ReadBytesExt;
                        let tag = src.read_u32::<byteorder::BigEndian>()?;
                        let result = match tag {
                            #(#deserialize_variants)*
                            _ => Err(xdr_codec::ErrorKind::InvalidEnum(tag as i32).into()),
                        };
                        result.map(|value| (value, 4usize))
                    }
                }
            }
        }
        _ => panic!("XdrCodec can only be derived for structs and enums"),
    }
        .into()
}
