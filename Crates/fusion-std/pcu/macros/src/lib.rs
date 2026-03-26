#![allow(non_snake_case)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Error,
    Expr,
    ExprBinary,
    ExprGroup,
    ExprLit,
    ExprMethodCall,
    ExprParen,
    ExprPath,
    ExprReturn,
    ExprUnary,
    FnArg,
    Ident,
    ItemFn,
    Lit,
    LitInt,
    Pat,
    PatIdent,
    Result,
    ReturnType,
    Signature,
    Stmt,
    Token,
    Type,
    UnOp,
    parse_macro_input,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    U8,
    U16,
    U32,
}

impl ScalarType {
    fn builder_method(self) -> &'static str {
        match self {
            Self::U8 => "bytes",
            Self::U16 => "half_words",
            Self::U32 => "words",
        }
    }

    fn runner_method(self) -> &'static str {
        match self {
            Self::U8 => "run_byte",
            Self::U16 => "run_half_word",
            Self::U32 => "run_word",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
        }
    }

    fn bit_width(self) -> u8 {
        match self {
            Self::U8 => 8,
            Self::U16 => 16,
            Self::U32 => 32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PcuPattern {
    BitReverse,
    BitInvert,
    Increment,
    ShiftLeft(u8),
    ShiftRight(u8),
    ByteSwap32,
}

impl PcuPattern {
    fn builder_tokens(self) -> TokenStream2 {
        match self {
            Self::BitReverse => quote! {
                .bit_reverse().expect("PCU builder should accept inferred bit-reverse pattern")
            },
            Self::BitInvert => quote! {
                .bit_invert().expect("PCU builder should accept inferred bit-invert pattern")
            },
            Self::Increment => quote! {
                .increment().expect("PCU builder should accept inferred increment pattern")
            },
            Self::ShiftLeft(bits) => quote! {
                .shift_left(#bits).expect("PCU builder should accept inferred left-shift pattern")
            },
            Self::ShiftRight(bits) => quote! {
                .shift_right(#bits).expect("PCU builder should accept inferred right-shift pattern")
            },
            Self::ByteSwap32 => quote! {
                .byte_swap32().expect("PCU builder should accept inferred byte-swap pattern")
            },
        }
    }

    fn descriptor(self) -> &'static str {
        match self {
            Self::BitReverse => "bit_reverse",
            Self::BitInvert => "bit_invert",
            Self::Increment => "increment",
            Self::ShiftLeft(_) => "shift_left",
            Self::ShiftRight(_) => "shift_right",
            Self::ByteSwap32 => "byte_swap32",
        }
    }
}

#[derive(Default)]
struct PcuArgs {
    threads: Option<u32>,
}

impl Parse for PcuArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key != "threads" {
                return Err(Error::new(
                    key.span(),
                    "unsupported #[PCU] option; only `threads = N` is supported right now",
                ));
            }
            let _: Token![=] = input.parse()?;
            let value: LitInt = input.parse()?;
            args.threads = Some(value.base10_parse()?);
            if input.is_empty() {
                break;
            }
            let _: Token![,] = input.parse()?;
        }
        Ok(args)
    }
}

#[proc_macro_attribute]
pub fn PCU(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as PcuArgs);
    let function = parse_macro_input!(item as ItemFn);
    match expand_pcu(args, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

fn expand_pcu(args: PcuArgs, function: ItemFn) -> Result<TokenStream2> {
    validate_signature(&function.sig)?;

    let input = parse_single_input(&function.sig)?;
    let output = parse_output_type(&function.sig)?;
    if input.ty != output {
        return Err(Error::new(
            function.sig.output.span(),
            "#[PCU] currently requires the output type to exactly match the input scalar type",
        ));
    }

    let body_expr = extract_body_expression(&function)?;
    let pattern = infer_pattern(input.ident.clone(), body_expr)?;
    validate_pattern_for_type(pattern, input.ty, body_expr.span())?;

    let attrs = function.attrs;
    let vis = function.vis;
    let sig = function.sig;
    let kernel_id = stable_kernel_id(sig.ident.to_string().as_str(), input.ty, pattern);
    let builder_method = Ident::new(input.ty.builder_method(), sig.ident.span());
    let runner_method = Ident::new(input.ty.runner_method(), sig.ident.span());
    let pattern_tokens = pattern.builder_tokens();
    let function_name = sig.ident.to_string();
    let kernel_entry = format!("{function_name}_{}", pattern.descriptor());
    let arg_ident = input.ident;

    let (setup_tokens, receiver_ident) = match args.threads {
        Some(threads) => {
            if threads == 0 {
                return Err(Error::new(
                    sig.ident.span(),
                    "#[PCU(threads = 0)] is invalid; thread count must be non-zero",
                ));
            }
            (
                quote! {
                    let __fusion_pcu_system = ::fusion_std::pcu::system_pcu();
                    let __fusion_pcu = __fusion_pcu_system
                        .profile()
                        .with_thread_count(#threads)
                        .expect("PCU thread count should be valid");
                },
                quote! { __fusion_pcu },
            )
        }
        None => (
            quote! {
                let __fusion_pcu = ::fusion_std::pcu::system_pcu();
            },
            quote! { __fusion_pcu },
        ),
    };

    Ok(quote! {
        #(#attrs)*
        #vis #sig {
            const __FUSION_PCU_KERNEL_ID: u32 = #kernel_id;
            #setup_tokens
            #receiver_ident
                .#builder_method(__FUSION_PCU_KERNEL_ID, #kernel_entry)
                #pattern_tokens
                .#runner_method(#arg_ident)
                .expect("PCU dispatch should complete")
        }
    })
}

fn validate_signature(signature: &Signature) -> Result<()> {
    if signature.asyncness.is_some() {
        return Err(Error::new(
            signature.asyncness.span(),
            "#[PCU] async functions are not supported yet; keep the substrate handle-based first",
        ));
    }
    if signature.constness.is_some() {
        return Err(Error::new(
            signature.constness.span(),
            "#[PCU] cannot be used on const functions",
        ));
    }
    if signature.unsafety.is_some() {
        return Err(Error::new(
            signature.unsafety.span(),
            "#[PCU] currently expects safe functions",
        ));
    }
    if signature.abi.is_some() {
        return Err(Error::new(
            signature.abi.span(),
            "#[PCU] cannot be used on extern functions",
        ));
    }
    if !signature.generics.params.is_empty() {
        return Err(Error::new(
            signature.generics.span(),
            "#[PCU] does not support generic functions yet",
        ));
    }
    Ok(())
}

struct ParsedInput {
    ident: Ident,
    ty: ScalarType,
}

fn parse_single_input(signature: &Signature) -> Result<ParsedInput> {
    if signature.inputs.len() != 1 {
        return Err(Error::new(
            signature.inputs.span(),
            "#[PCU] currently supports exactly one scalar input",
        ));
    }
    let FnArg::Typed(argument) = signature.inputs.first().expect("one input should exist") else {
        return Err(Error::new(
            signature.inputs.span(),
            "#[PCU] methods are not supported yet",
        ));
    };
    let Pat::Ident(PatIdent { ident, .. }) = argument.pat.as_ref() else {
        return Err(Error::new(
            argument.pat.span(),
            "#[PCU] expects a plain identifier parameter",
        ));
    };
    let ty = parse_scalar_type(argument.ty.as_ref())?;
    Ok(ParsedInput {
        ident: ident.clone(),
        ty,
    })
}

fn parse_output_type(signature: &Signature) -> Result<ScalarType> {
    let ReturnType::Type(_, ty) = &signature.output else {
        return Err(Error::new(
            signature.output.span(),
            "#[PCU] requires an explicit scalar return type",
        ));
    };
    parse_scalar_type(ty.as_ref())
}

fn parse_scalar_type(ty: &Type) -> Result<ScalarType> {
    let Type::Path(path) = ty else {
        return Err(Error::new(
            ty.span(),
            "#[PCU] currently supports only `u8`, `u16`, or `u32` scalar types",
        ));
    };
    let Some(ident) = path.path.get_ident() else {
        return Err(Error::new(
            ty.span(),
            "#[PCU] currently supports only `u8`, `u16`, or `u32` scalar types",
        ));
    };
    match ident.to_string().as_str() {
        "u8" => Ok(ScalarType::U8),
        "u16" => Ok(ScalarType::U16),
        "u32" => Ok(ScalarType::U32),
        _ => Err(Error::new(
            ty.span(),
            "#[PCU] currently supports only `u8`, `u16`, or `u32` scalar types",
        )),
    }
}

fn extract_body_expression(function: &ItemFn) -> Result<&Expr> {
    if function.block.stmts.len() != 1 {
        return Err(Error::new(
            function.block.span(),
            "#[PCU] currently expects one simple expression body",
        ));
    }
    let statement = function
        .block
        .stmts
        .first()
        .expect("one statement should exist");
    match statement {
        Stmt::Expr(expr, _) => match expr {
            Expr::Return(ExprReturn {
                expr: Some(inner), ..
            }) => Ok(inner.as_ref()),
            _ => Ok(expr),
        },
        _ => Err(Error::new(
            statement.span(),
            "#[PCU] currently expects one simple expression body",
        )),
    }
}

fn infer_pattern(argument: Ident, expr: &Expr) -> Result<PcuPattern> {
    let expr = strip_expression_wrappers(expr);
    match expr {
        Expr::MethodCall(method) => infer_method_pattern(&argument, method),
        Expr::Unary(unary) => infer_unary_pattern(&argument, unary),
        Expr::Binary(binary) => infer_binary_pattern(&argument, binary),
        _ => Err(Error::new(
            expr.span(),
            "#[PCU] could not infer a supported semantic stream pattern from this function body",
        )),
    }
}

fn infer_method_pattern(argument: &Ident, method: &ExprMethodCall) -> Result<PcuPattern> {
    if !expr_is_ident(
        strip_expression_wrappers(method.receiver.as_ref()),
        argument,
    ) {
        return Err(Error::new(
            method.receiver.span(),
            "#[PCU] only direct transforms over the function parameter are supported right now",
        ));
    }
    match method.method.to_string().as_str() {
        "reverse_bits" if method.args.is_empty() => Ok(PcuPattern::BitReverse),
        "swap_bytes" if method.args.is_empty() => Ok(PcuPattern::ByteSwap32),
        "wrapping_add"
            if method.args.len() == 1 && literal_u8(method.args.first().unwrap()) == Some(1) =>
        {
            Ok(PcuPattern::Increment)
        }
        _ => Err(Error::new(
            method.span(),
            "#[PCU] this method call is not a supported PCU stream pattern yet",
        )),
    }
}

fn infer_unary_pattern(argument: &Ident, unary: &ExprUnary) -> Result<PcuPattern> {
    if !matches!(unary.op, UnOp::Not(_)) {
        return Err(Error::new(
            unary.op.span(),
            "#[PCU] only bitwise NOT unary transforms are supported right now",
        ));
    }
    if !expr_is_ident(strip_expression_wrappers(unary.expr.as_ref()), argument) {
        return Err(Error::new(
            unary.expr.span(),
            "#[PCU] only direct transforms over the function parameter are supported right now",
        ));
    }
    Ok(PcuPattern::BitInvert)
}

fn infer_binary_pattern(argument: &Ident, binary: &ExprBinary) -> Result<PcuPattern> {
    if !expr_is_ident(strip_expression_wrappers(binary.left.as_ref()), argument) {
        return Err(Error::new(
            binary.left.span(),
            "#[PCU] only direct transforms over the function parameter are supported right now",
        ));
    }
    let Some(bits) = literal_u8(binary.right.as_ref()) else {
        return Err(Error::new(
            binary.right.span(),
            "#[PCU] shift counts must currently be integer literals",
        ));
    };
    match &binary.op {
        syn::BinOp::Shl(_) => Ok(PcuPattern::ShiftLeft(bits)),
        syn::BinOp::Shr(_) => Ok(PcuPattern::ShiftRight(bits)),
        _ => Err(Error::new(
            binary.op.span(),
            "#[PCU] only left and right shift binary transforms are supported right now",
        )),
    }
}

fn validate_pattern_for_type(
    pattern: PcuPattern,
    ty: ScalarType,
    span: proc_macro2::Span,
) -> Result<()> {
    match pattern {
        PcuPattern::ShiftLeft(bits) | PcuPattern::ShiftRight(bits) => {
            if bits == 0 || bits > ty.bit_width() {
                return Err(Error::new(
                    span,
                    format!(
                        "#[PCU] shift count must be in 1..={} for {}",
                        ty.bit_width(),
                        ty.name()
                    ),
                ));
            }
        }
        PcuPattern::ByteSwap32 if ty != ScalarType::U32 => {
            return Err(Error::new(
                span,
                "#[PCU] byte-swap lowering currently expects `u32`",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn strip_expression_wrappers(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Paren(ExprParen { expr, .. }) => expr.as_ref(),
            Expr::Group(ExprGroup { expr, .. }) => expr.as_ref(),
            _ => return expr,
        };
    }
}

fn expr_is_ident(expr: &Expr, ident: &Ident) -> bool {
    let Expr::Path(ExprPath { path, .. }) = expr else {
        return false;
    };
    path.is_ident(ident)
}

fn literal_u8(expr: &Expr) -> Option<u8> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(int), ..
    }) = strip_expression_wrappers(expr)
    else {
        return None;
    };
    int.base10_parse().ok()
}

fn stable_kernel_id(name: &str, ty: ScalarType, pattern: PcuPattern) -> u32 {
    const OFFSET: u32 = 0x811c9dc5;
    const PRIME: u32 = 0x0100_0193;

    let descriptor = format!("{name}:{}:{}", ty.name(), pattern.descriptor());
    let mut hash = OFFSET;
    for byte in descriptor.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn infers_increment_pattern_from_wrapping_add() {
        let function: ItemFn = parse_quote! {
            fn increment_word(value: u32) -> u32 {
                value.wrapping_add(1)
            }
        };
        let input = parse_single_input(&function.sig).expect("input should parse");
        let body = extract_body_expression(&function).expect("body should parse");

        assert_eq!(
            infer_pattern(input.ident, body).expect("pattern should infer"),
            PcuPattern::Increment
        );
    }

    #[test]
    fn infers_shift_left_pattern() {
        let function: ItemFn = parse_quote! {
            fn shift_word(value: u32) -> u32 {
                value << 4
            }
        };
        let input = parse_single_input(&function.sig).expect("input should parse");
        let body = extract_body_expression(&function).expect("body should parse");

        assert_eq!(
            infer_pattern(input.ident, body).expect("pattern should infer"),
            PcuPattern::ShiftLeft(4)
        );
    }

    #[test]
    fn rejects_threads_zero() {
        let function: ItemFn = parse_quote! {
            fn increment_word(value: u32) -> u32 {
                value.wrapping_add(1)
            }
        };
        let error = expand_pcu(PcuArgs { threads: Some(0) }, function)
            .expect_err("threads=0 should be rejected");

        assert!(error.to_string().contains("thread count"));
    }
}
