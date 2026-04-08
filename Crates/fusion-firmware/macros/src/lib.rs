use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{
    format_ident,
    quote,
};
use syn::parse::{
    Parse,
    ParseStream,
};
use syn::spanned::Spanned;
use syn::{
    Error,
    Expr,
    FnArg,
    ItemFn,
    ReturnType,
    Token,
    Type,
    parse_macro_input,
};

struct FusionFirmwareMainArgs {
    policy: Option<Expr>,
}

impl Parse for FusionFirmwareMainArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self, Error> {
        if input.is_empty() {
            return Ok(Self { policy: None });
        }
        let key: syn::Ident = input.parse()?;
        if key != "policy" {
            return Err(Error::new(
                key.span(),
                "#[fusion_firmware_main] only supports `policy = <expr>` right now",
            ));
        }
        let _: Token![=] = input.parse()?;
        let policy = input.parse::<Expr>()?;
        if !input.is_empty() {
            return Err(Error::new(
                input.span(),
                "#[fusion_firmware_main] only supports one `policy = <expr>` argument",
            ));
        }
        Ok(Self {
            policy: Some(policy),
        })
    }
}

#[proc_macro_attribute]
pub fn fusion_firmware_main(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as FusionFirmwareMainArgs);
    let function = parse_macro_input!(item as ItemFn);
    match expand_fusion_firmware_main(args, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_fusion_firmware_main(
    args: FusionFirmwareMainArgs,
    function: ItemFn,
) -> Result<TokenStream2, Error> {
    validate_signature(&function)?;

    let attrs = function.attrs;
    let vis = function.vis;
    let mut sig = function.sig;
    let block = function.block;
    let original_ident = sig.ident.clone();
    let user_ident = format_ident!("__fusion_firmware_user_{}", original_ident);
    let bootstrap_ident = format_ident!("__fusion_bootstrap");
    let argument_count = sig.inputs.len();
    sig.ident = user_ident.clone();
    let policy_expr = args.policy.map_or_else(
        || quote! { ::fusion_firmware::RootCourierPolicy::disabled() },
        |policy| quote! { #policy },
    );

    let invocation = match argument_count {
        0 => quote! { #user_ident() },
        1 => quote! {
            #user_ident(&#bootstrap_ident)
        },
        _ => unreachable!("signature already validated"),
    };

    Ok(quote! {
        #(#attrs)*
        #vis #sig #block
        #[cfg(not(target_os = "none"))]
        const _: () = {
            ::core::compile_error!(
                "#[fusion_firmware_main] currently requires one bare-metal PAL entry surface"
            );
        };

        #[cfg(target_os = "none")]
        #[::fusion_firmware::__fusion_pal_entry::__rt::entry]
        fn #original_ident() -> ! {
            let __fusion_root_policy = #policy_expr;
            let #bootstrap_ident =
                ::fusion_firmware::sys::hal::runtime::bootstrap_root_execution_with_policy(
                    __fusion_root_policy,
                )
                .expect("Fusion firmware entry should bootstrap root execution");
            match ::fusion_firmware::sys::hal::runtime::run_root_fiber(move || {
                #invocation
            }) {
                Ok(__fusion_never) => match __fusion_never {},
                Err(_) => panic!("Fusion firmware entry should run the root managed fiber"),
            }
        }
    })
}

fn validate_signature(function: &ItemFn) -> Result<(), Error> {
    let signature = &function.sig;
    if signature.asyncness.is_some() {
        return Err(Error::new(
            signature.asyncness.span(),
            "#[fusion_firmware_main] does not support async entry functions",
        ));
    }
    if signature.constness.is_some() {
        return Err(Error::new(
            signature.constness.span(),
            "#[fusion_firmware_main] cannot be used on const functions",
        ));
    }
    if signature.unsafety.is_some() {
        return Err(Error::new(
            signature.unsafety.span(),
            "#[fusion_firmware_main] expects a safe function",
        ));
    }
    if signature.abi.is_some() {
        return Err(Error::new(
            signature.abi.span(),
            "#[fusion_firmware_main] cannot be used on extern functions",
        ));
    }
    if !signature.generics.params.is_empty() {
        return Err(Error::new(
            signature.generics.span(),
            "#[fusion_firmware_main] does not support generic entry functions",
        ));
    }
    match signature.inputs.len() {
        0 => {}
        1 => validate_single_argument(&signature.inputs[0])?,
        _ => {
            return Err(Error::new(
                signature.inputs.span(),
                "#[fusion_firmware_main] expects `fn main() -> !` or `fn main(&FirmwareBootstrapContext) -> !`",
            ));
        }
    }

    if let ReturnType::Type(_, ty) = &signature.output {
        if !matches!(ty.as_ref(), Type::Never(_)) {
            return Err(Error::new(
                ty.span(),
                "#[fusion_firmware_main] expects an entry function returning `!`",
            ));
        }
    } else {
        return Err(Error::new(
            signature.output.span(),
            "#[fusion_firmware_main] expects an entry function returning `!`",
        ));
    }

    Ok(())
}

fn validate_single_argument(argument: &FnArg) -> Result<(), Error> {
    match argument {
        FnArg::Receiver(receiver) => Err(Error::new(
            receiver.span(),
            "#[fusion_firmware_main] cannot be used on methods",
        )),
        FnArg::Typed(argument) => {
            if let Type::Reference(reference) = argument.ty.as_ref() {
                if let Type::Path(path) = reference.elem.as_ref() {
                    if let Some(segment) = path.path.segments.last() {
                        if segment.ident == "FirmwareBootstrapContext" {
                            return Ok(());
                        }
                    }
                }
            }
            Err(Error::new(
                argument.ty.span(),
                "#[fusion_firmware_main] expects the single parameter to be `&FirmwareBootstrapContext`",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn parse_empty_args_defaults_policy_to_none() {
        let args: FusionFirmwareMainArgs =
            syn::parse2(TokenStream2::new()).expect("empty args should parse");
        assert!(args.policy.is_none());
    }

    #[test]
    fn parse_policy_expr_argument() {
        let args: FusionFirmwareMainArgs = syn::parse2(quote!(
            policy = ::fusion_firmware::RootCourierPolicy::disabled()
        ))
        .expect("policy expression should parse");
        assert!(args.policy.is_some());
    }

    #[test]
    fn validate_zero_arg_entry_signature() {
        let function: ItemFn = parse_quote! {
            fn main() -> ! {
                loop {}
            }
        };
        validate_signature(&function).expect("plain main signature should validate");
    }

    #[test]
    fn validate_bootstrap_context_entry_signature() {
        let function: ItemFn = parse_quote! {
            fn main(_bootstrap: &FirmwareBootstrapContext) -> ! {
                loop {}
            }
        };
        validate_signature(&function).expect("bootstrap-context main signature should validate");
    }

    #[test]
    fn expanded_entry_uses_policy_bootstrap_path() {
        let args: FusionFirmwareMainArgs = syn::parse2(quote!(
            policy = ::fusion_firmware::RootCourierPolicy::disabled()
        ))
        .expect("policy args should parse");
        let function: ItemFn = parse_quote! {
            fn main() -> ! {
                loop {}
            }
        };
        let expanded =
            expand_fusion_firmware_main(args, function).expect("expansion should succeed");
        let text = expanded.to_string();
        assert!(text.contains("bootstrap_root_execution_with_policy"));
        assert!(text.contains("RootCourierPolicy :: disabled"));
        assert!(text.contains("run_root_fiber"));
        assert!(text.contains("move ||"));
    }
}
