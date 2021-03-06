extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use std::collections::BTreeSet;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, Data, DeriveInput, GenericParam, Generics, Token};

#[proc_macro_derive(CandidType)]
pub fn derive_idl_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = add_trait_bounds(input.generics);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let (ty_body, ser_body) = match input.data {
        Data::Enum(ref data) => enum_from_ast(&name, &data.variants),
        Data::Struct(ref data) => {
            let (ty, idents) = struct_from_ast(&data.fields);
            (ty, serialize_struct(&idents))
        }
        Data::Union(_) => unimplemented!("doesn't derive union type"),
    };
    let gen = quote! {
        impl #impl_generics ::candid::types::CandidType for #name #ty_generics #where_clause {
            fn _ty() -> ::candid::types::Type {
                #ty_body
            }
            fn id() -> ::candid::types::TypeId { ::candid::types::TypeId::of::<#name #ty_generics>() }

            fn idl_serialize<__S>(&self, __serializer: __S) -> ::std::result::Result<(), __S::Error>
                where
                __S: ::candid::types::Serializer,
                {
                    #ser_body
                }
        }
    };
    //panic!(gen.to_string());
    TokenStream::from(gen)
}

#[inline]
fn idl_hash(id: &str) -> u32 {
    let mut s: u32 = 0;
    for c in id.as_bytes().iter() {
        s = s.wrapping_mul(223).wrapping_add(*c as u32);
    }
    s
}

struct Variant {
    real_ident: syn::Ident,
    renamed_ident: syn::Ident,
    hash: u32,
    ty: Tokens,
    members: Vec<Ident>,
}
enum Style {
    Struct,
    Tuple,
    Unit,
}
impl Variant {
    fn style(&self) -> Style {
        if self.members.is_empty() {
            return Style::Unit;
        };
        match self.members[0] {
            Ident::Named(_) => Style::Struct,
            Ident::Unnamed(_) => Style::Tuple,
        }
    }
    fn to_pattern(&self) -> (Tokens, Vec<Tokens>) {
        match self.style() {
            Style::Unit => (quote! {}, Vec::new()),
            Style::Struct => {
                let id: Vec<_> = self.members.iter().map(|ident| ident.to_token()).collect();
                (
                    quote! {
                        {#(ref #id),*}
                    },
                    id,
                )
            }
            Style::Tuple => {
                let id: Vec<_> = self
                    .members
                    .iter()
                    .map(|ident| {
                        let ident = ident.to_string();
                        let var = format!("__field{}", ident);
                        syn::parse_str(&var).unwrap()
                    })
                    .collect();
                (
                    quote! {
                        (#(ref #id),*)
                    },
                    id,
                )
            }
        }
    }
}

fn enum_from_ast(
    name: &syn::Ident,
    variants: &Punctuated<syn::Variant, Token![,]>,
) -> (Tokens, Tokens) {
    let mut fs: Vec<_> = variants
        .iter()
        .map(|variant| {
            let id = variant.ident.clone();
            let (renamed_ident, hash) = match get_rename_attrs(&variant.attrs) {
                Some(ref rename) => (syn::parse_str(rename).unwrap(), idl_hash(rename)),
                None => (id.clone(), idl_hash(&id.to_string())),
            };
            let (ty, idents) = struct_from_ast(&variant.fields);
            Variant {
                real_ident: id,
                renamed_ident,
                hash,
                ty,
                members: idents,
            }
        })
        .collect();
    let unique: BTreeSet<_> = fs.iter().map(|Variant { hash, .. }| hash).collect();
    assert_eq!(unique.len(), fs.len());
    fs.sort_unstable_by_key(|Variant { hash, .. }| *hash);

    let id = fs
        .iter()
        .map(|Variant { renamed_ident, .. }| renamed_ident.to_string());
    let ty = fs.iter().map(|Variant { ty, .. }| ty);
    let ty_gen = quote! {
        ::candid::types::Type::Variant(
            vec![
                #(::candid::types::Field {
                    id: ::candid::types::Label::Named(#id.to_owned()),
                    ty: #ty }
                ),*
            ]
        )
    };

    let id = fs.iter().map(|Variant { real_ident, .. }| {
        syn::parse_str::<Tokens>(&format!("{}::{}", name, real_ident)).unwrap()
    });
    let index = 0..fs.len() as u64;
    let (pattern, members): (Vec<_>, Vec<_>) = fs
        .iter()
        .map(|f| {
            let (pattern, id) = f.to_pattern();
            (
                pattern,
                quote! {
                    #(::candid::types::Compound::serialize_element(&mut ser, #id)?;)*
                },
            )
        })
        .unzip();
    let variant_gen = quote! {
        match *self {
            #(#id #pattern => {
                let mut ser = __serializer.serialize_variant(#index)?;
                #members
            }),*
        };
        Ok(())
    };
    (ty_gen, variant_gen)
}

fn serialize_struct(idents: &[Ident]) -> Tokens {
    let id = idents.iter().map(|ident| ident.to_token());
    quote! {
        let mut ser = __serializer.serialize_struct()?;
        #(::candid::types::Compound::serialize_element(&mut ser, &self.#id)?;)*
        Ok(())
    }
}

fn struct_from_ast(fields: &syn::Fields) -> (Tokens, Vec<Ident>) {
    match *fields {
        syn::Fields::Named(ref fields) => {
            let (fs, idents) = fields_from_ast(&fields.named);
            (quote! { ::candid::types::Type::Record(#fs) }, idents)
        }
        syn::Fields::Unnamed(ref fields) => {
            let (fs, idents) = fields_from_ast(&fields.unnamed);
            if idents.len() == 1 {
                let newtype = derive_type(&fields.unnamed[0].ty);
                (quote! { #newtype }, idents)
            } else {
                (quote! { ::candid::types::Type::Record(#fs) }, idents)
            }
        }
        syn::Fields::Unit => (quote! { ::candid::types::Type::Null }, Vec::new()),
    }
}

#[derive(Clone)]
enum Ident {
    Named(syn::Ident),
    Unnamed(u32),
}
impl Ident {
    fn to_token(&self) -> Tokens {
        match self {
            Ident::Named(ident) => quote! { #ident },
            Ident::Unnamed(ref i) => syn::parse_str::<Tokens>(&format!("{}", i)).unwrap(),
        }
    }
}
impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Ident::Named(ref ident) => f.write_fmt(format_args!("{}", ident.to_string())),
            Ident::Unnamed(ref i) => f.write_fmt(format_args!("{}", (*i).to_string())),
        }
    }
}

struct Field {
    real_ident: Ident,
    renamed_ident: Ident,
    hash: u32,
    ty: Tokens,
}

fn get_serde_meta_items(attr: &syn::Attribute) -> Result<Vec<syn::NestedMeta>, ()> {
    if !attr.path.is_ident("serde") {
        return Ok(Vec::new());
    }
    match attr.parse_meta() {
        Ok(syn::Meta::List(meta)) => Ok(meta.nested.into_iter().collect()),
        _ => Err(()),
    }
}

fn get_rename_attrs(attrs: &[syn::Attribute]) -> Option<String> {
    use syn::Meta::{List, NameValue};
    use syn::NestedMeta::Meta;
    for item in attrs.iter().flat_map(get_serde_meta_items).flatten() {
        match &item {
            // #[serde(rename = "foo")]
            Meta(NameValue(m)) if m.path.is_ident("rename") => {
                if let syn::Lit::Str(lit) = &m.lit {
                    return Some(lit.value());
                }
            }
            // #[serde(rename(serialize = "foo"))]
            Meta(List(metas)) if metas.path.is_ident("rename") => {
                for item in metas.nested.iter() {
                    match item {
                        Meta(NameValue(m)) if m.path.is_ident("serialize") => {
                            if let syn::Lit::Str(lit) = &m.lit {
                                return Some(lit.value());
                            }
                        }
                        _ => continue,
                    }
                }
            }
            _ => continue,
        }
    }
    None
}

fn fields_from_ast(fields: &Punctuated<syn::Field, syn::Token![,]>) -> (Tokens, Vec<Ident>) {
    let mut fs: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, field)| {
            let (real_ident, renamed_ident, hash) = match field.ident {
                Some(ref ident) => {
                    let real_ident = Ident::Named(ident.clone());
                    match get_rename_attrs(&field.attrs) {
                        Some(ref renamed) => {
                            let renamed_ident = Ident::Named(syn::parse_str(renamed).unwrap());
                            (real_ident, renamed_ident, idl_hash(renamed))
                        }
                        None => (real_ident.clone(), real_ident, idl_hash(&ident.to_string())),
                    }
                }
                None => (Ident::Unnamed(i as u32), Ident::Unnamed(i as u32), i as u32),
            };
            Field {
                real_ident,
                renamed_ident,
                hash,
                ty: derive_type(&field.ty),
            }
        })
        .collect();
    let unique: BTreeSet<_> = fs.iter().map(|Field { hash, .. }| hash).collect();
    assert_eq!(unique.len(), fs.len());
    fs.sort_unstable_by_key(|Field { hash, .. }| *hash);

    let id = fs
        .iter()
        .map(|Field { renamed_ident, .. }| match renamed_ident {
            Ident::Named(ref id) => {
                let name = id.to_string();
                quote! { ::candid::types::Label::Named(#name.to_string()) }
            }
            Ident::Unnamed(ref i) => quote! { ::candid::types::Label::Id(#i) },
        });
    let ty = fs.iter().map(|Field { ty, .. }| ty);
    let ty_gen = quote! {
        vec![
            #(::candid::types::Field {
                id: #id,
                ty: #ty }
            ),*
        ]
    };
    let idents: Vec<Ident> = fs
        .iter()
        .map(|Field { real_ident, .. }| real_ident.clone())
        .collect();
    (ty_gen, idents)
}

fn derive_type(t: &syn::Type) -> Tokens {
    quote! {
        <#t as ::candid::types::CandidType>::ty()
    }
}

fn add_trait_bounds(mut generics: Generics) -> Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(ref mut type_param) = *param {
            let bound = syn::parse_str("::candid::types::CandidType").unwrap();
            type_param.bounds.push(bound);
        }
    }
    generics
}
