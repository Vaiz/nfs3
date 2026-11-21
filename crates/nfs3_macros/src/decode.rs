use proc_macro2::TokenStream;
use syn::DeriveInput;

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    crate::checked::expand(input, crate::checked::Direction::Decode)
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
            expand(&input).unwrap_err();
            crate::encode::expand(&input).unwrap_err();
        }
    }
}
