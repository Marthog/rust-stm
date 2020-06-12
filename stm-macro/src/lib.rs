extern crate proc_macro;

use proc_macro::*;
use proc_macro2::Span;
use syn::{ReturnType, Token, Type};
use syn::{
    fold::Fold
};

struct Stm {
    outer_fn: bool,
}

impl Stm{
    pub fn new() -> Stm {
        Stm{ outer_fn: true, }
    }


    pub fn fold(&mut self, input: TokenStream) -> TokenStream {
        if let Ok(item_fn) = syn::parse(input.clone()) {
            let item_fn = self.fold_item_fn(item_fn);
            quote::quote!(#item_fn).into()
        } else {
            panic!("#[stm] attribute can only be applied to functions and methods.")
        }
    }
}

impl Fold for Stm {
    fn fold_item_fn(&mut self, func: syn::ItemFn) -> syn::ItemFn {
        if !self.outer_fn { return func; }

        let sig = syn::Signature {
            output: self.fold_return_type(func.sig.output),
            ..func.sig
        };

        self.outer_fn = false;

        let inner = self.fold_block(*func.block);
        let block = Box::new(make_fn_block(&inner));

        syn::ItemFn { sig, block, ..func }
    }

    fn fold_impl_item_method(&mut self, i: syn::ImplItemMethod) -> syn::ImplItemMethod {
        if !self.outer_fn { return i; }

        let sig = syn::Signature {
            output: self.fold_return_type(i.sig.output),
            ..i.sig
        };

        self.outer_fn = false;

        let inner = self.fold_block(i.block);
        let block = make_fn_block(&inner);

        syn::ImplItemMethod { sig, block, ..i }
    }

    fn fold_trait_item_method(&mut self, mut i: syn::TraitItemMethod) -> syn::TraitItemMethod {
        if !self.outer_fn { return i; }

        let sig = syn::Signature {
            output: self.fold_return_type(i.sig.output),
            ..i.sig
        };

        self.outer_fn = false;

        let default = i.default.take().map(|block| {
            let inner = self.fold_block(block);
            make_fn_block(&inner)
        });


        syn::TraitItemMethod { sig, default, ..i }
    }

    fn fold_return_type(&mut self, ret: syn::ReturnType) -> syn::ReturnType {
        if !self.outer_fn { return ret; }

        let (arrow, ret) = match ret {
            ReturnType::Default	=> (arrow(), unit()),
            ReturnType::Type(arrow, ty) => (arrow, *ty),
        };
        let new_ret = syn::parse2(quote::quote!{
            crate::StmResult<#ret>
        }).unwrap();
        ReturnType::Type(arrow, new_ret)
    }

    fn fold_expr_return(&mut self, i: syn::ExprReturn) -> syn::ExprReturn {
        let ok = match &i.expr {
            Some(expr)  => ok(expr),
            None        => ok_unit(),
        };
        syn::ExprReturn { expr: Some(Box::new(ok)), ..i }
    }
}

fn make_fn_block(inner: &syn::Block) -> syn::Block {
    syn::parse2(quote::quote! {{
        let __ret = #inner;

 //       #[allow(unreachable_code)]
 //       <_ as ::stm_core::StmResult>::new(__ret)
        std::ops::Try::from_ok(__ret)
    }}).unwrap()
}


#[proc_macro_attribute]
pub fn stm(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let output = Stm::new().fold(input);
    //println!("{}", output);
    output
}


fn arrow() -> syn::token::RArrow {
    Token![->](Span::call_site())
}

fn unit() -> Type {
    syn::parse_str("()").unwrap()
}

fn ok(expr: &syn::Expr) -> syn::Expr {
    syn::parse2(quote::quote!(std::ops::Try::from_ok(#expr))).unwrap()
}

fn ok_unit() -> syn::Expr {
    syn::parse2(quote::quote!(std::ops::Try::from_ok(()))).unwrap()
}