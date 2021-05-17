use crate::helper_structs::ParenthesizedTokens;
use crate::helpers::call_site_ident;
use proc_macro2::{Ident, Span, TokenStream};
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Field, Fields, ItemEnum, Type};

pub fn derive_discriminant_impl(input_item: TokenStream) -> syn::Result<TokenStream> {
	let input = syn::parse2::<DeriveInput>(input_item).unwrap();

	let mut data = match input.data {
		Data::Enum(data) => data,
		_ => return Err(syn::Error::new(Span::call_site(), "Tried to derive a discriminant for non-enum")),
	};

	let mut is_child = vec![];
	let mut attr_errs = vec![];

	for var in &mut data.variants {
		if var.attrs.iter().any(|a| a.path.is_ident("child")) {
			match var.fields.len() {
				1 => {
					let Field { ty, .. } = var.fields.iter_mut().next().unwrap();
					match ty {
						Type::Path(type_path) => {
							let last = type_path.path.segments.last_mut().unwrap();
							last.ident = call_site_ident(format!("{}Discriminant", last.ident));
						}
						_ => unimplemented!("#[child] on variants with payload types besides paths is not supported for now"),
					}
					is_child.push(true);
				}
				n => unimplemented!("#[child] on variants with {} fields is not supported for now", n),
			}
		} else {
			var.fields = Fields::Unit;
			is_child.push(false);
		}
		let mut retain = vec![];
		for (i, a) in var.attrs.iter_mut().enumerate() {
			if a.path.is_ident("discriminant_attr") {
				match syn::parse2::<ParenthesizedTokens>(a.tokens.clone()) {
					Ok(ParenthesizedTokens { tokens, .. }) => {
						let attr: Attribute = syn::parse_quote! {
							#[#tokens]
						};
						*a = attr;
						retain.push(i);
					}
					Err(e) => {
						attr_errs.push(syn::Error::new(a.span(), e));
					}
				}
			}
		}
		var.attrs = var.attrs.iter().enumerate().filter_map(|(i, x)| retain.contains(&i).then(|| x.clone())).collect();
	}

	let discriminant_derives = input.attrs.iter().cloned().filter_map(|mut a| {
		a.path.is_ident("discriminant_derive").then(|| {
			a.path.segments.last_mut().unwrap().ident = call_site_ident("derive");
			a
		})
	});

	let discriminant_attributes = input.attrs.iter().cloned().filter_map(|a| {
		let a_span = a.span();
		a.path
			.is_ident("discriminant_attr")
			.then(|| match syn::parse2::<ParenthesizedTokens>(a.tokens) {
				Ok(ParenthesizedTokens { tokens, .. }) => {
					let attr: Attribute = syn::parse_quote! {
						#[#tokens]
					};
					Some(attr)
				}
				Err(e) => {
					attr_errs.push(syn::Error::new(a_span, e));
					None
				}
			})
			.and_then(|opt| opt)
	});

	let attrs = discriminant_derives.chain(discriminant_attributes).collect::<Vec<Attribute>>();

	if !attr_errs.is_empty() {
		return Err(attr_errs
			.into_iter()
			.reduce(|mut l, r| {
				l.combine(r);
				l
			})
			.unwrap());
	}

	let discriminant = ItemEnum {
		attrs,
		vis: input.vis,
		enum_token: data.enum_token,
		ident: call_site_ident(format!("{}Discriminant", input.ident)),
		generics: input.generics,
		brace_token: data.brace_token,
		variants: data.variants,
	};

	let input_type = &input.ident;
	let discriminant_type = &discriminant.ident;
	let variant = &discriminant.variants.iter().map(|var| &var.ident).collect::<Vec<&Ident>>();

	let (pattern, value) = is_child
		.into_iter()
		.map(|b| {
			(
				if b {
					quote::quote! { (x) }
				} else {
					quote::quote! { { .. } }
				},
				b.then(|| quote::quote! { (x.to_discriminant()) }).unwrap_or_default(),
			)
		})
		.unzip::<_, _, Vec<_>, Vec<_>>();

	let res = quote::quote! {
		#discriminant

		impl ToDiscriminant for #input_type {
			type Discriminant = #discriminant_type;

			fn to_discriminant(&self) -> #discriminant_type {
				match self {
					#(
						#input_type::#variant #pattern => #discriminant_type::#variant #value
					),*
				}
			}
		}

		impl From<&#input_type> for #discriminant_type {
			fn from(x: &#input_type) -> #discriminant_type {
				x.to_discriminant()
			}
		}
	};

	Ok(res)
}