#![doc = include_str!("../README.md")]

extern crate proc_macro;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Data, DataEnum, DeriveInput, Expr, Fields, FieldsNamed, FieldsUnnamed, Ident, Index,
    Lit, Meta, Variant, parse_macro_input,
};

/// Helper function to parse #[xdr(value)] attribute
fn parse_xdr_value(attrs: &[Attribute]) -> Option<u32> {
    for attr in attrs {
        if attr.path().is_ident("xdr") {
            if let Meta::List(meta_list) = &attr.meta {
                if let Ok(Expr::Lit(syn::ExprLit {
                    lit: Lit::Int(lit_int),
                    ..
                })) = meta_list.parse_args::<Expr>()
                {
                    if let Ok(value) = lit_int.base10_parse::<u32>() {
                        return Some(value);
                    }
                }
            }
        }
    }
    None
}

/// Generate field operations for named struct fields
struct NamedFieldsGenerator<'a> {
    fields: &'a FieldsNamed,
}

impl<'a> NamedFieldsGenerator<'a> {
    const fn new(fields: &'a FieldsNamed) -> Self {
        Self { fields }
    }

    fn pack_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.named.iter().map(|f| {
            let name = &f.ident;
            quote! {
                total_write += self.#name.pack(out)?;
            }
        })
    }

    fn packed_size_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.named.iter().map(|f| {
            let name = &f.ident;
            quote! {
                total_size += nfs3_types::xdr_codec::Pack::packed_size(&self.#name);
            }
        })
    }

    fn unpack_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.named.iter().map(|f| {
            let name = &f.ident;
            quote! {
                let (#name, read_bytes) = nfs3_types::xdr_codec::Unpack::unpack(input)?;
                total_read += read_bytes;
            }
        })
    }

    fn struct_construction_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.named.iter().map(|f| {
            let name = &f.ident;
            quote! { #name, }
        })
    }
}

/// Generate field operations for unnamed struct fields
struct UnnamedFieldsGenerator<'a> {
    fields: &'a FieldsUnnamed,
}

impl<'a> UnnamedFieldsGenerator<'a> {
    const fn new(fields: &'a FieldsUnnamed) -> Self {
        Self { fields }
    }

    fn pack_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.unnamed.iter().enumerate().map(|(i, _)| {
            let index = Index::from(i);
            quote! {
                total_write += self.#index.pack(out)?;
            }
        })
    }

    fn packed_size_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.unnamed.iter().enumerate().map(|(i, _)| {
            let index = Index::from(i);
            quote! {
                total_size += nfs3_types::xdr_codec::Pack::packed_size(&self.#index);
            }
        })
    }

    fn unpack_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        self.fields.unnamed.iter().enumerate().map(|(i, _)| {
            let var_name = Ident::new(&format!("field_{i}"), proc_macro2::Span::call_site());
            quote! {
                let (#var_name, read_bytes) = nfs3_types::xdr_codec::Unpack::unpack(input)?;
                total_read += read_bytes;
            }
        })
    }

    fn struct_construction_fields(&self) -> impl Iterator<Item = TokenStream2> + '_ {
        (0..self.fields.unnamed.len()).map(|i| {
            let var_name = Ident::new(&format!("field_{i}"), proc_macro2::Span::call_site());
            quote! { #var_name }
        })
    }
}

/// Generate XDR codec implementations for struct types
fn generate_struct_impl(name: &Ident, generics: &syn::Generics, fields: &Fields) -> TokenStream2 {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    match fields {
        Fields::Named(named_fields) => {
            let generator = NamedFieldsGenerator::new(named_fields);
            let pack_fields = generator.pack_fields();
            let packed_size_fields = generator.packed_size_fields();
            let unpack_fields = generator.unpack_fields();
            let struct_fields = generator.struct_construction_fields();

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
            let generator = UnnamedFieldsGenerator::new(unnamed_fields);
            let pack_fields = generator.pack_fields();
            let packed_size_fields = generator.packed_size_fields();
            let unpack_fields = generator.unpack_fields();
            let struct_fields = generator.struct_construction_fields();

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
        Fields::Unit => {
            quote! {
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
            }
        }
    }
}

/// Validate complex enum variant fields
fn validate_complex_enum_variant(variant: &Variant) -> Result<(), String> {
    match &variant.fields {
        Fields::Unit => Ok(()),
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => Ok(()),
        _ => Err(format!(
            "Complex enum variant '{}' must be either unit or have exactly one unnamed field",
            variant.ident
        )),
    }
}

/// Generate pack implementation for complex enum variants
fn generate_complex_enum_pack_variant(variant: &Variant) -> TokenStream2 {
    let ident = &variant.ident;
    let xdr_value = parse_xdr_value(&variant.attrs).unwrap_or_else(|| {
        panic!("Complex enum variant '{ident}' must have #[xdr(value)] attribute");
    });

    match &variant.fields {
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
        _ => panic!("Invalid complex enum variant: {ident}"),
    }
}

/// Generate `packed_size` implementation for complex enum variants
fn generate_complex_enum_packed_size_variant(variant: &Variant) -> TokenStream2 {
    let ident = &variant.ident;

    match &variant.fields {
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
        _ => panic!("Invalid complex enum variant: {ident}"),
    }
}

/// Generate unpack implementation for complex enum variants
fn generate_complex_enum_unpack_variant(variant: &Variant) -> TokenStream2 {
    let ident = &variant.ident;
    let xdr_value = parse_xdr_value(&variant.attrs).unwrap_or_else(|| {
        panic!("Complex enum variant '{ident}' must have #[xdr(value)] attribute");
    });

    match &variant.fields {
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
        _ => panic!("Invalid complex enum variant: {ident}"),
    }
}

/// Generate XDR codec implementations for simple enums (all unit variants)
fn generate_simple_enum_impl(
    name: &Ident,
    generics: &syn::Generics,
    data: &DataEnum,
) -> TokenStream2 {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

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

/// Generate XDR codec implementations for complex enums (has data variants)
fn generate_complex_enum_impl(
    name: &Ident,
    generics: &syn::Generics,
    data: &DataEnum,
) -> TokenStream2 {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    // Validate all variants first
    for variant in &data.variants {
        if let Err(err) = validate_complex_enum_variant(variant) {
            panic!("{}", err);
        }
    }

    let pack_variants = data.variants.iter().map(generate_complex_enum_pack_variant);
    let packed_size_variants = data
        .variants
        .iter()
        .map(generate_complex_enum_packed_size_variant);
    let unpack_variants = data
        .variants
        .iter()
        .map(generate_complex_enum_unpack_variant);

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
}

/// Generate XDR codec implementations for enum types
fn generate_enum_impl(name: &Ident, generics: &syn::Generics, data: &DataEnum) -> TokenStream2 {
    // Check if this is a simple enum (all unit variants) or complex enum (has data variants)
    let has_data_variants = data
        .variants
        .iter()
        .any(|v| !matches!(v.fields, Fields::Unit));

    if has_data_variants {
        generate_complex_enum_impl(name, generics, data)
    } else {
        generate_simple_enum_impl(name, generics, data)
    }
}

#[proc_macro_derive(XdrCodec, attributes(xdr))]
#[allow(clippy::missing_panics_doc)]
pub fn derive_xdr_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let generics = &input.generics;

    let result = match &input.data {
        Data::Struct(data_struct) => generate_struct_impl(name, generics, &data_struct.fields),
        Data::Enum(data_enum) => generate_enum_impl(name, generics, data_enum),
        Data::Union(_) => panic!("XdrCodec can only be derived for structs and enums"),
    };

    result.into()
}
