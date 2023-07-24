#![feature(try_blocks)]

use {
    proc_macro2::{Span, TokenStream},
    quote::{format_ident, quote},
    std::{
        env, fs, io,
        sync::atomic::{AtomicUsize, Ordering},
    },
    syn::{
        parse::{Parse, ParseStream},
        parse_macro_input,
        punctuated::Punctuated,
        Error, FnArg, Token, Type,
    },
};

#[derive(Debug)]
struct Input {
    sig: (Punctuated<FnArg, Token![,]>, Type),
    block: TokenStream,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let args = Punctuated::parse_separated_nonempty(input)?;
        let _: Token![=>] = input.parse()?;
        let ret = input.parse()?;
        let _: Token![=>] = input.parse()?;

        Ok(Self {
            sig: (args, ret),
            block: input.parse()?,
        })
    }
}

#[proc_macro]
pub fn js_impl(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    static UNIQUE: AtomicUsize = AtomicUsize::new(0);

    let unique = UNIQUE.fetch_add(1, Ordering::SeqCst);
    let ident = format_ident!("__some_prefix_{unique}");
    let Input {
        sig: (args, ret),
        block,
    } = parse_macro_input!(input as Input);

    let names = args.iter().map(|arg| match arg {
        FnArg::Receiver(_) => unreachable!(),
        FnArg::Typed(ty) => &ty.pat,
    });
    let err: io::Result<_> = try {
        let path = env::current_dir()?.join("_jsauto_cache");
        let _ = fs::create_dir(&path);

        let names = names.clone();
        fs::write(
            path.join(&format!("{ident}.js")),
            quote!(
          module.exports.#ident = async function(#(#names),*) {
              #block
          }
      )
                .to_string(),
        )?;
    };

    if let Err(err) = err {
        return Error::new(Span::call_site(), format!("{err}: lmao"))
            .to_compile_error()
            .into();
    }

    let path = format!("/_jsauto_cache/{ident}.js");
    let ext = quote!(
      #[wasm_bindgen(module = #path)]
      extern "C" {
          async fn #ident(#args) -> wasm_bindgen::JsValue;
      }
  );

    let into = if quote!(#ret).to_string() == quote!(JsValue).to_string() {
        quote!( async move { #ident(#(#names),*).await } )
    } else {
        quote! {
        async move { serde_wasm_bindgen::from_value::<#ret>(#ident(#(#names),*).await).unwrap() }
    }
    };

    quote!(
      #ext #into
  )
        .into()
}
