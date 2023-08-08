#![feature(try_blocks)]

use {
    proc_macro2::{Ident, Span, TokenStream},
    quote::{format_ident, quote},
    std::{
        env, fs, io,
        sync::atomic::{AtomicUsize, Ordering},
    },
    syn::{
        parse::{Parse, ParseStream},
        parse_macro_input,
        punctuated::Punctuated,
        Error, Token,
    },
};

#[derive(Debug)]
struct Input {
    sig: Punctuated<Ident, Token![,]>,
    block: TokenStream,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let sig = Punctuated::parse_separated_nonempty(input).unwrap_or_default();
        let _: Token![=>] = input.parse()?;

        Ok(Self { sig, block: input.parse()? })
    }
}

#[proc_macro]
pub fn js_impl(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    static UNIQUE: AtomicUsize = AtomicUsize::new(0);

    let unique = UNIQUE.fetch_add(1, Ordering::SeqCst);
    let ident = format_ident!("__some_prefix_{unique}");
    let Input { sig, block } = parse_macro_input!(input as Input);

    let err: io::Result<_> = try {
        let path = env::current_dir()?.join("_jsauto_cache");
        let _ = fs::create_dir(&path);

        fs::write(
            path.join(&format!("{ident}.js")),
            quote!(
                module.exports.#ident = async function(#sig) {
                    #block
                }
            )
            .to_string(),
        )?;
    };

    if let Err(err) = err {
        return Error::new(Span::call_site(), format!("{err}: lmao")).to_compile_error().into();
    }

    let path = format!("/_jsauto_cache/{ident}.js");
    let (ser, de) = (sig.iter(), sig.iter());
    quote!(
        #[wasm_bindgen(module = #path)]
        extern "C" {
            async fn #ident(#(#ser: wasm_bindgen::JsValue),*) -> wasm_bindgen::JsValue;
        }

        async move {
            __FromJsMacro::__from(#ident(#(__IntoJsMacro::__into(#de)),*).await)
        }
    )
    .into()
}
