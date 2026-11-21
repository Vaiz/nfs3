#![doc = include_str!("../README.md")]

extern crate proc_macro;
mod checked;
mod decode;
mod encode;
mod tags;

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Derive owner-aware decoding independently of encoding.
#[proc_macro_derive(XdrDecode, attributes(xdr))]
pub fn derive_xdr_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    decode::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive checked segmented encoding independently of decoding.
#[proc_macro_derive(XdrEncode, attributes(xdr))]
pub fn derive_xdr_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    encode::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derive the checked pair with independent directional bounds.
#[proc_macro_derive(XdrCodec, attributes(xdr))]
pub fn derive_xdr_codec(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let result = (|| {
        let encode = encode::expand(&input)?;
        let decode = decode::expand(&input)?;
        Ok::<_, syn::Error>(quote!(#encode #decode))
    })();
    result.unwrap_or_else(syn::Error::into_compile_error).into()
}
