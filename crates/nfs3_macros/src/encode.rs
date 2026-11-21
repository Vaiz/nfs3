use proc_macro2::TokenStream;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    crate::checked::expand(input, crate::checked::Direction::Encode)
}
