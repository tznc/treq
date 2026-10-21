use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input,
    punctuated::Punctuated,
    FnArg, GenericArgument, Ident, Item, ItemFn, ItemMod, ItemStruct, LitStr, Pat, PathArguments,
    ReturnType, Token, Type,
};

#[proc_macro_attribute]
pub fn command(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);
    strip_tauri_test_command_attr(&mut func);
    let trampoline = generate_trampoline(&func);
    quote! {
        #func
        #trampoline
    }
    .into()
}

#[proc_macro_attribute]
pub fn dispatch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as DispatchArgs);
    let struct_item = parse_macro_input!(item as ItemStruct);
    let struct_name = &struct_item.ident;

    let mut arms = Vec::new();

    for (name_lit, handler_path) in &args.overrides {
        arms.push(quote! {
            #name_lit => #handler_path(__args),
        });
    }

    for path in &args.commands {
        let cmd_name = path_to_command_name(path);
        let trampoline = path_to_trampoline(path);
        arms.push(quote! {
            #cmd_name => #trampoline(__args),
        });
    }

    for name_lit in &args.no_op {
        arms.push(quote! {
            #name_lit => Ok(serde_json::Value::Null),
        });
    }

    for name_lit in &args.not_implemented {
        arms.push(quote! {
            #name_lit => Err(format!("not_implemented: '{}' — migrate to a core equivalent", #name_lit)),
        });
    }

    quote! {
        #struct_item

        impl tauri_test::Dispatcher for #struct_name {
            fn dispatch(
                command: &str,
                args: serde_json::Value,
            ) -> ::std::result::Result<serde_json::Value, ::std::string::String> {
                let __args = &args;
                match command {
                    #(#arms)*
                    _ => Err(format!("unknown_command: '{}'", command)),
                }
            }
        }
    }
    .into()
}

#[proc_macro_attribute]
pub fn setup(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as SetupArgs);
    let struct_item = parse_macro_input!(item as ItemStruct);

    if !matches!(struct_item.fields, syn::Fields::Unit) {
        return syn::Error::new_spanned(
            &struct_item,
            "#[tauri_test::setup] only supports unit structs",
        )
        .to_compile_error()
        .into();
    }

    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(value) => PathBuf::from(value),
        Err(err) => {
            return syn::Error::new_spanned(
                &struct_item,
                format!("missing CARGO_MANIFEST_DIR: {err}"),
            )
            .to_compile_error()
            .into()
        }
    };

    let scan = match scan_crate(&manifest_dir) {
        Ok(scan) => scan,
        Err(err) => {
            return syn::Error::new_spanned(&struct_item, err)
                .to_compile_error()
                .into()
        }
    };

    if let Err(err) = emit_target_loader(&manifest_dir) {
        return syn::Error::new_spanned(
            &struct_item,
            format!("failed to write target loader: {err}"),
        )
        .to_compile_error()
        .into();
    }

    let dispatch_impl =
        match generate_setup_impl(&struct_item, &scan, args.init.as_ref(), args.state.as_ref()) {
            Ok(tokens) => tokens,
            Err(err) => {
                return syn::Error::new_spanned(&struct_item, err)
                    .to_compile_error()
                    .into()
            }
        };

    quote! {
        #struct_item
        #dispatch_impl
    }
    .into()
}

struct DispatchArgs {
    commands: Vec<syn::Path>,
    overrides: Vec<(LitStr, syn::Path)>,
    no_op: Vec<LitStr>,
    not_implemented: Vec<LitStr>,
}

impl Parse for DispatchArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut commands = Vec::new();
        let mut overrides = Vec::new();
        let mut no_op = Vec::new();
        let mut not_implemented = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "commands" => {
                    let content;
                    syn::bracketed!(content in input);
                    let paths: Punctuated<syn::Path, Token![,]> =
                        content.parse_terminated(syn::Path::parse, Token![,])?;
                    commands = paths.into_iter().collect();
                }
                "overrides" => {
                    let content;
                    syn::bracketed!(content in input);
                    while !content.is_empty() {
                        let inner;
                        syn::parenthesized!(inner in content);
                        let name: LitStr = inner.parse()?;
                        inner.parse::<Token![,]>()?;
                        let path: syn::Path = inner.parse()?;
                        overrides.push((name, path));
                        if content.peek(Token![,]) {
                            content.parse::<Token![,]>()?;
                        }
                    }
                }
                "no_op" => {
                    let content;
                    syn::bracketed!(content in input);
                    let lits: Punctuated<LitStr, Token![,]> =
                        content.parse_terminated(|p| p.parse::<LitStr>(), Token![,])?;
                    no_op = lits.into_iter().collect();
                }
                "not_implemented" => {
                    let content;
                    syn::bracketed!(content in input);
                    let lits: Punctuated<LitStr, Token![,]> =
                        content.parse_terminated(|p| p.parse::<LitStr>(), Token![,])?;
                    not_implemented = lits.into_iter().collect();
                }
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown dispatch option: {other}"),
                    ))
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(DispatchArgs {
            commands,
            overrides,
            no_op,
            not_implemented,
        })
    }
}

struct SetupArgs {
    init: Option<syn::Path>,
    state: Option<syn::Path>,
}

impl Parse for SetupArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut init = None;
        let mut state = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "init" => init = Some(input.parse::<syn::Path>()?),
                "state" => state = Some(input.parse::<syn::Path>()?),
                other => {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("unknown setup option: {other}; expected `init` or `state`"),
                    ))
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self { init, state })
    }
}

#[derive(Clone)]
struct CommandSpec {
    module_path: Vec<String>,
    function: ItemFn,
}

#[derive(Clone)]
struct FunctionSpec {
    module_path: Vec<String>,
    function: ItemFn,
}

struct ScanResult {
    commands: Vec<CommandSpec>,
    functions: Vec<FunctionSpec>,
}

fn generate_setup_impl(
    _struct_item: &ItemStruct,
    scan: &ScanResult,
    init_path: Option<&syn::Path>,
    state: Option<&syn::Path>,
) -> Result<proc_macro2::TokenStream, String> {
    let init = resolve_init_spec(scan, init_path)?;
    let dispatch_arms = build_command_arms(&scan.commands)?;
    let init_fn = generate_init_function(init);
    let state_napi = generate_state_napi(state);

    Ok(quote! {
        #init_fn

        #[::napi_derive::napi]
        pub async fn invoke(
            cmd: ::std::string::String,
            args: serde_json::Value,
        ) -> napi::Result<serde_json::Value> {
            __tauri_test_init_once().map_err(napi::Error::from_reason)?;

            let __args = &args;
            match cmd.as_str() {
                #(#dispatch_arms)*
                _ => Err(napi::Error::from_reason(format!("unknown_command: '{}'", cmd))),
            }
        }

        #state_napi
    })
}

fn generate_state_napi(state: Option<&syn::Path>) -> proc_macro2::TokenStream {
    let Some(ty) = state else {
        return quote! {};
    };

    quote! {
        #[::napi_derive::napi]
        pub fn get_app_state() -> napi::Result<serde_json::Value> {
            __tauri_test_init_once().map_err(napi::Error::from_reason)?;
            tauri_test::state::to_json::<#ty>().map_err(napi::Error::from_reason)
        }
    }
}

fn build_command_arms(commands: &[CommandSpec]) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let mut seen = HashSet::new();
    let mut arms = Vec::new();

    for command in commands {
        let name = command.function.sig.ident.to_string();
        if !seen.insert(name.clone()) {
            return Err(format!(
                "duplicate #[tauri::command] function name found: {name}"
            ));
        }

        let command_name = LitStr::new(&name, command.function.sig.ident.span());
        let fn_path = module_fn_path(&command.module_path, &command.function.sig.ident);

        let mut arg_extractions = Vec::new();
        let mut call_args = Vec::new();

        for input in &command.function.sig.inputs {
            let FnArg::Typed(pat_type) = input else {
                continue;
            };

            let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
                return Err(format!(
                    "unsupported command parameter pattern in {}",
                    command.function.sig.ident
                ));
            };

            let param = &pat_ident.ident;
            let json_key = snake_to_camel(&param.to_string());
            let ty = pat_type.ty.as_ref();

            if is_state_type(ty) {
                let inner_ty = extract_state_inner(ty);
                arg_extractions.push(quote! {
                    let #param: ::tauri::State<'static, #inner_ty> =
                        tauri_test::state::get_tauri_state::<#inner_ty>()?;
                });
            } else {
                arg_extractions.push(generate_arg_extraction(param, &json_key, ty));
            }

            call_args.push(quote! { #param });
        }

        let result = generate_invoke_result(
            &fn_path,
            &call_args,
            &command.function.sig.output,
            command.function.sig.asyncness.is_some(),
        );

        arms.push(quote! {
            #command_name => {
                let __fut = async {
                    #(#arg_extractions)*
                    #result
                };
                let __typed: ::std::result::Result<serde_json::Value, ::std::string::String> =
                    __fut.await;
                __typed.map_err(napi::Error::from_reason)
            }
        });
    }

    Ok(arms)
}

fn generate_invoke_result(
    fn_path: &syn::Path,
    call_args: &[proc_macro2::TokenStream],
    return_type: &ReturnType,
    is_async: bool,
) -> proc_macro2::TokenStream {
    let call = if is_async {
        quote! { #fn_path(#(#call_args),*).await }
    } else {
        quote! { #fn_path(#(#call_args),*) }
    };

    match return_type {
        ReturnType::Default => quote! {
            {
                #call;
                Ok(serde_json::Value::Null)
            }
        },
        ReturnType::Type(_, ty) => {
            if unwrap_result_ok_type(ty).is_some() {
                quote! {
                    {
                        let __result = #call.map_err(|e| e.to_string())?;
                        serde_json::to_value(__result).map_err(|e| e.to_string())
                    }
                }
            } else if is_unit_type(ty) {
                quote! {
                    {
                        #call;
                        Ok(serde_json::Value::Null)
                    }
                }
            } else {
                quote! {
                    {
                        let __result = #call;
                        serde_json::to_value(__result).map_err(|e| e.to_string())
                    }
                }
            }
        }
    }
}

fn generate_init_function(init: Option<FunctionSpec>) -> proc_macro2::TokenStream {
    let Some(init) = init else {
        return quote! {
            fn __tauri_test_init_once() -> ::std::result::Result<(), ::std::string::String> {
                Ok(())
            }
        };
    };

    let fn_path = module_fn_path(&init.module_path, &init.function.sig.ident);
    let init_body = generate_init_body(&fn_path, &init.function.sig.output);

    quote! {
        fn __tauri_test_init_once() -> ::std::result::Result<(), ::std::string::String> {
            static INIT: ::std::sync::Once = ::std::sync::Once::new();
            static ERROR: ::std::sync::OnceLock<::std::string::String> = ::std::sync::OnceLock::new();

            INIT.call_once(|| {
                tauri_test::state::clear();
                if let Err(err) = (|| -> ::std::result::Result<(), ::std::string::String> {
                    #init_body
                    Ok(())
                })() {
                    let _ = ERROR.set(err);
                }
            });

            if let Some(err) = ERROR.get() {
                return Err(err.clone());
            }

            Ok(())
        }
    }
}

fn generate_init_body(fn_path: &syn::Path, output: &ReturnType) -> proc_macro2::TokenStream {
    match output {
        ReturnType::Default => quote! {
            #fn_path();
        },
        ReturnType::Type(_, ty) => {
            if let Some(ok_ty) = unwrap_result_ok_type(ty) {
                if is_unit_type(&ok_ty) {
                    quote! {
                        #fn_path().map_err(|e| e.to_string())?;
                    }
                } else {
                    let registration = generate_state_registration(quote! { __state }, &ok_ty);
                    quote! {
                        let __state = #fn_path().map_err(|e| e.to_string())?;
                        #registration
                    }
                }
            } else if is_unit_type(ty) {
                quote! {
                    #fn_path();
                }
            } else {
                let registration = generate_state_registration(quote! { __state }, ty);
                quote! {
                    let __state = #fn_path();
                    #registration
                }
            }
        }
    }
}

fn generate_state_registration(
    value: proc_macro2::TokenStream,
    ty: &Type,
) -> proc_macro2::TokenStream {
    if let Type::Tuple(tuple) = ty {
        let indices = (0..tuple.elems.len()).map(syn::Index::from);
        quote! {
            #(tauri_test::state::register(#value.#indices);)*
        }
    } else {
        quote! {
            tauri_test::state::register(#value);
        }
    }
}

fn resolve_init_spec(
    scan: &ScanResult,
    init_path: Option<&syn::Path>,
) -> Result<Option<FunctionSpec>, String> {
    let Some(init_path) = init_path else {
        return Ok(None);
    };

    let target_segments: Vec<String> = init_path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();

    let mut matches = Vec::new();
    for function in &scan.functions {
        let mut full_path = function.module_path.clone();
        full_path.push(function.function.sig.ident.to_string());

        if target_segments.len() == 1 {
            if full_path.last() == target_segments.last() {
                matches.push(function.clone());
            }
        } else if full_path.ends_with(&target_segments) {
            matches.push(function.clone());
        }
    }

    match matches.len() {
        1 => Ok(matches.into_iter().next()),
        0 => Err(format!(
            "could not resolve init function `{}`",
            quote! { #init_path }
        )),
        _ => Err(format!(
            "init function `{}` is ambiguous",
            quote! { #init_path }
        )),
    }
}

fn scan_crate(manifest_dir: &Path) -> Result<ScanResult, String> {
    let src_dir = manifest_dir.join("src");
    let root_file = if src_dir.join("lib.rs").exists() {
        src_dir.join("lib.rs")
    } else {
        src_dir.join("main.rs")
    };

    if !root_file.exists() {
        return Err(format!(
            "no src/lib.rs or src/main.rs found in {}",
            manifest_dir.display()
        ));
    }

    let mut commands = Vec::new();
    let mut functions = Vec::new();
    let mut visited = HashSet::new();
    scan_module_file(
        &root_file,
        &src_dir,
        &mut Vec::new(),
        &mut visited,
        &mut commands,
        &mut functions,
    )?;

    Ok(ScanResult {
        commands,
        functions,
    })
}

fn scan_module_file(
    file_path: &Path,
    module_dir: &Path,
    module_path: &mut Vec<String>,
    visited: &mut HashSet<PathBuf>,
    commands: &mut Vec<CommandSpec>,
    functions: &mut Vec<FunctionSpec>,
) -> Result<(), String> {
    let canonical = fs::canonicalize(file_path)
        .map_err(|err| format!("failed to resolve {}: {err}", file_path.display()))?;
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }

    let source = fs::read_to_string(&canonical)
        .map_err(|err| format!("failed to read {}: {err}", canonical.display()))?;
    let file = syn::parse_file(&source)
        .map_err(|err| format!("failed to parse {}: {err}", canonical.display()))?;

    scan_items(
        &file.items,
        module_dir,
        module_path,
        visited,
        commands,
        functions,
    )
}

fn scan_items(
    items: &[Item],
    current_dir: &Path,
    module_path: &mut Vec<String>,
    visited: &mut HashSet<PathBuf>,
    commands: &mut Vec<CommandSpec>,
    functions: &mut Vec<FunctionSpec>,
) -> Result<(), String> {
    for item in items {
        match item {
            Item::Fn(function) => {
                functions.push(FunctionSpec {
                    module_path: module_path.clone(),
                    function: function.clone(),
                });

                if has_tauri_command_attr(&function.attrs) {
                    commands.push(CommandSpec {
                        module_path: module_path.clone(),
                        function: function.clone(),
                    });
                }
            }
            Item::Mod(item_mod) => {
                scan_nested_module(
                    item_mod,
                    current_dir,
                    module_path,
                    visited,
                    commands,
                    functions,
                )?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn scan_nested_module(
    item_mod: &ItemMod,
    current_dir: &Path,
    module_path: &mut Vec<String>,
    visited: &mut HashSet<PathBuf>,
    commands: &mut Vec<CommandSpec>,
    functions: &mut Vec<FunctionSpec>,
) -> Result<(), String> {
    if !module_is_enabled(item_mod) {
        return Ok(());
    }

    module_path.push(item_mod.ident.to_string());

    let result = if let Some((_, items)) = &item_mod.content {
        scan_items(
            items,
            current_dir,
            module_path,
            visited,
            commands,
            functions,
        )
    } else {
        let file_path = resolve_module_file(current_dir, &item_mod.ident)?;
        let next_dir = child_dir(&file_path, current_dir);
        scan_module_file(
            &file_path,
            &next_dir,
            module_path,
            visited,
            commands,
            functions,
        )
    };

    module_path.pop();
    result
}

/// Where a module file's own `mod x;` items live: beside a `mod.rs`,
/// else in the directory named after the file (`a.rs` -> `a/`).
fn child_dir(file_path: &Path, current_dir: &Path) -> PathBuf {
    let parent = file_path.parent().unwrap_or(current_dir);
    match file_path.file_stem().and_then(|s| s.to_str()) {
        Some("mod") | None => parent.to_path_buf(),
        Some(stem) => parent.join(stem),
    }
}

fn resolve_module_file(current_dir: &Path, ident: &Ident) -> Result<PathBuf, String> {
    let direct = current_dir.join(format!("{ident}.rs"));
    if direct.exists() {
        return Ok(direct);
    }

    let nested = current_dir.join(ident.to_string()).join("mod.rs");
    if nested.exists() {
        return Ok(nested);
    }

    Err(format!(
        "could not resolve module file for `{ident}` in {}",
        current_dir.display()
    ))
}

fn module_is_enabled(item_mod: &ItemMod) -> bool {
    for attr in &item_mod.attrs {
        if !attr.path().is_ident("cfg") {
            continue;
        }

        let tokens = attr.meta.to_token_stream().to_string();
        if tokens.contains("test")
            || tokens.contains("feature")
            || tokens.contains("debug_assertions")
        {
            continue;
        }
    }
    true
}

fn emit_target_loader(manifest_dir: &Path) -> Result<(), String> {
    let cargo_toml = fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .map_err(|err| format!("failed to read Cargo.toml: {err}"))?;
    let value: toml::Value = cargo_toml
        .parse()
        .map_err(|err| format!("failed to parse Cargo.toml: {err}"))?;

    let lib_name = value
        .get("lib")
        .and_then(|lib| lib.get("name"))
        .and_then(toml::Value::as_str)
        .or_else(|| {
            value
                .get("package")
                .and_then(|pkg| pkg.get("name"))
                .and_then(toml::Value::as_str)
        })
        .ok_or_else(|| "could not determine crate library name".to_string())?;

    let js = format!(
        r#"const fs = require("fs");
const path = require("path");

const manifestDir = path.resolve(__dirname, "..");
const libraryStem = {lib_name:?}.replace(/-/g, "_");
const libraryNames = process.platform === "win32"
  ? [libraryStem + ".dll"]
  : process.platform === "darwin"
    ? ["lib" + libraryStem + ".dylib"]
    : ["lib" + libraryStem + ".so"];

function candidatePaths() {{
  const roots = [];
  let current = manifestDir;
  for (let depth = 0; depth < 6; depth += 1) {{
    roots.push(current);
    const parent = path.dirname(current);
    if (parent === current) {{
      break;
    }}
    current = parent;
  }}

  const out = [];
  for (const root of roots) {{
    const targetDir = path.join(root, "target");
    for (const profile of ["debug", "release"]) {{
      for (const name of libraryNames) {{
        out.push(path.join(targetDir, profile, name));
      }}

      if (!fs.existsSync(targetDir)) {{
        continue;
      }}

      for (const entry of fs.readdirSync(targetDir)) {{
        for (const name of libraryNames) {{
          out.push(path.join(targetDir, entry, profile, name));
        }}
      }}
    }}
  }}

  return [...new Set(out)];
}}

let lastError = null;
for (const filename of candidatePaths()) {{
  if (!fs.existsSync(filename)) {{
    continue;
  }}

  try {{
    process.dlopen(module, filename);
    lastError = null;
    break;
  }} catch (error) {{
    lastError = error;
  }}
}}

if (lastError) {{
  throw lastError;
}}

if (!module.exports || typeof module.exports.invoke !== "function") {{
  throw new Error(
    "tauri-test: could not load invoke() from " + __dirname +
      ". Build src-tauri with `cargo build` before running tests."
  );
}}
"#,
    );

    let target_dir = manifest_dir.join("target");
    fs::create_dir_all(&target_dir)
        .map_err(|err| format!("failed to create {}: {err}", target_dir.display()))?;
    fs::write(target_dir.join("index.js"), js)
        .map_err(|err| format!("failed to write loader: {err}"))?;
    Ok(())
}

fn module_fn_path(module_path: &[String], ident: &Ident) -> syn::Path {
    let mut segments = module_path
        .iter()
        .map(|segment| Ident::new(segment, Span::call_site()))
        .collect::<Vec<_>>();
    segments.push(ident.clone());
    syn::parse_quote!(#(#segments)::*)
}

fn has_tauri_command_attr(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let segments = &attr.path().segments;
        matches!(segments.len(), 2)
            && segments
                .first()
                .map(|segment| segment.ident == "tauri")
                .unwrap_or(false)
            && segments
                .last()
                .map(|segment| segment.ident == "command")
                .unwrap_or(false)
    })
}

fn strip_tauri_test_command_attr(func: &mut ItemFn) {
    func.attrs.retain(|attr| {
        let segments = &attr.path().segments;
        let Some(last) = segments.last() else {
            return true;
        };

        if last.ident != "command" {
            return true;
        }

        match segments.len() {
            1 => false,
            2 => segments
                .first()
                .map(|segment| segment.ident != "tauri_test")
                .unwrap_or(true),
            _ => true,
        }
    });
}

fn generate_trampoline(func: &ItemFn) -> proc_macro2::TokenStream {
    let fn_name = &func.sig.ident;
    let trampoline_name = format_ident!("__tauri_test_dispatch_{}", fn_name);

    let mut arg_extractions = Vec::new();
    let mut call_args = Vec::new();

    for input in &func.sig.inputs {
        let FnArg::Typed(pat_type) = input else {
            continue;
        };

        let Pat::Ident(pat_ident) = pat_type.pat.as_ref() else {
            continue;
        };
        let param_name = &pat_ident.ident;
        let json_key = snake_to_camel(&param_name.to_string());
        let ty = pat_type.ty.as_ref();

        if is_state_type(ty) {
            let inner_ty = extract_state_inner(ty);
            arg_extractions.push(quote! {
                let #param_name: ::tauri::State<'static, #inner_ty> =
                    tauri_test::state::get_tauri_state::<#inner_ty>()?;
            });
        } else {
            arg_extractions.push(generate_arg_extraction(param_name, &json_key, ty));
        }
        call_args.push(quote! { #param_name });
    }

    let result_serialization = generate_result_handling(fn_name, &call_args, &func.sig.output);

    quote! {
        #[doc(hidden)]
        pub fn #trampoline_name(
            __args: &serde_json::Value,
        ) -> ::std::result::Result<serde_json::Value, ::std::string::String> {
            #(#arg_extractions)*
            #result_serialization
        }
    }
}

fn generate_arg_extraction(param: &Ident, json_key: &str, ty: &Type) -> proc_macro2::TokenStream {
    let key_lit = LitStr::new(json_key, Span::call_site());

    if is_string_type(ty) {
        quote! {
            let #param: ::std::string::String = tauri_test::args::get_str(__args, #key_lit)?;
        }
    } else if is_bool_type(ty) {
        let err_msg = format!("Missing argument: {json_key}");
        quote! {
            let #param: bool = __args
                .get(#key_lit)
                .and_then(|v| v.as_bool())
                .ok_or_else(|| #err_msg.to_string())?;
        }
    } else if let Some(int_ty) = integer_type(ty) {
        match int_ty {
            IntType::I64 => quote! {
                let #param: i64 = tauri_test::args::get_i64(__args, #key_lit)?;
            },
            IntType::U64 => {
                let err_msg = format!("Missing argument: {json_key}");
                quote! {
                    let #param: u64 = __args
                        .get(#key_lit)
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| #err_msg.to_string())?;
                }
            }
            IntType::I32 => quote! {
                let #param: i32 = tauri_test::args::get_i64(__args, #key_lit)? as i32;
            },
            IntType::U32 => quote! {
                let #param: u32 = tauri_test::args::get_i64(__args, #key_lit)? as u32;
            },
            IntType::Usize => quote! {
                let #param: usize = tauri_test::args::get_i64(__args, #key_lit)? as usize;
            },
        }
    } else if is_app_handle_type(ty) {
        quote! {
            let #param: #ty = tauri_test::runtime::expect_app_handle()?;
        }
    } else if is_webview_window_type(ty) {
        quote! {
            let #param: #ty = tauri_test::runtime::expect_webview_window()?;
        }
    } else if is_option_type(ty) {
        quote! {
            let #param: #ty = serde_json::from_value(
                __args.get(#key_lit).cloned().unwrap_or(serde_json::Value::Null)
            ).map_err(|e| e.to_string())?;
        }
    } else {
        quote! {
            let #param: #ty = serde_json::from_value::<#ty>(
                __args.get(#key_lit).cloned().unwrap_or(serde_json::Value::Null)
            ).map_err(|e| e.to_string())?;
        }
    }
}

fn generate_result_handling(
    fn_name: &Ident,
    call_args: &[proc_macro2::TokenStream],
    return_type: &ReturnType,
) -> proc_macro2::TokenStream {
    match return_type {
        ReturnType::Default => quote! {
            #fn_name(#(#call_args),*);
            Ok(serde_json::Value::Null)
        },
        ReturnType::Type(_, ty) => {
            if unwrap_result_ok_type(ty).is_some() {
                quote! {
                    let __result = #fn_name(#(#call_args),*)
                        .map_err(|e| e.to_string())?;
                    serde_json::to_value(__result).map_err(|e| e.to_string())
                }
            } else if is_unit_type(ty) {
                quote! {
                    #fn_name(#(#call_args),*);
                    Ok(serde_json::Value::Null)
                }
            } else {
                quote! {
                    let __result = #fn_name(#(#call_args),*);
                    serde_json::to_value(__result).map_err(|e| e.to_string())
                }
            }
        }
    }
}

fn snake_to_camel(s: &str) -> String {
    let s = s.trim_start_matches('_');
    let mut out = String::new();
    let mut cap = false;
    for ch in s.chars() {
        if ch == '_' {
            cap = true;
        } else if cap {
            out.extend(ch.to_uppercase());
            cap = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn path_to_command_name(path: &syn::Path) -> LitStr {
    let last = path.segments.last().unwrap();
    let ident = last.ident.to_string();
    let command_name = ident
        .strip_suffix("_impl")
        .filter(|name| !name.is_empty())
        .unwrap_or(&ident);
    LitStr::new(command_name, last.ident.span())
}

fn path_to_trampoline(path: &syn::Path) -> proc_macro2::TokenStream {
    let mut segs = path.segments.iter().collect::<Vec<_>>();
    let last = segs.pop().unwrap();
    let trampoline_ident = format_ident!("__tauri_test_dispatch_{}", last.ident);

    if segs.is_empty() {
        quote! { #trampoline_ident }
    } else {
        let prefix = segs.iter().map(|s| &s.ident);
        quote! { #(#prefix ::)* #trampoline_ident }
    }
}

fn last_segment_ident(ty: &Type) -> Option<&Ident> {
    if let Type::Path(tp) = ty {
        tp.path.segments.last().map(|s| &s.ident)
    } else {
        None
    }
}

fn is_string_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "String")
}

fn is_bool_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "bool")
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(t) if t.elems.is_empty())
}

fn is_option_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "Option")
}

fn is_state_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "State")
}

fn is_app_handle_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "AppHandle")
}

fn is_webview_window_type(ty: &Type) -> bool {
    last_segment_ident(ty).is_some_and(|id| id == "WebviewWindow")
}

fn result_ok_type(ty: &Type) -> Option<Type> {
    let Type::Path(tp) = ty else {
        return None;
    };
    let last = tp.path.segments.last()?;
    if last.ident != "Result" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    for argument in &arguments.args {
        if let GenericArgument::Type(inner_ty) = argument {
            return Some(inner_ty.clone());
        }
    }
    None
}

/// `Result` plus common single-type-arg aliases (`type AppResult<T> = Result<T, String>`).
fn unwrap_result_ok_type(ty: &Type) -> Option<Type> {
    result_ok_type(ty).or_else(|| single_type_arg_result_alias_ok(ty))
}

/// Known limitation: a proc macro cannot resolve type aliases, so this is
/// name-based. A single-generic user type that merely *ends* in `Result`
/// (`QueryResult<T>`) is treated as a `Result` and mis-unwrapped. Accepted
/// trade-off — the alias convention is far more common than the collision.
fn single_type_arg_result_alias_ok(ty: &Type) -> Option<Type> {
    let Type::Path(tp) = ty else {
        return None;
    };
    let last = tp.path.segments.last()?;
    let ident = last.ident.to_string();
    if ident == "Result" || ident == "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    let mut only: Option<Type> = None;
    for argument in &arguments.args {
        if let GenericArgument::Type(inner_ty) = argument {
            if only.is_some() {
                return None;
            }
            only = Some(inner_ty.clone());
        }
    }
    let inner = only?;
    // `FooResult<T>` convention; avoids serializing `serde`’s tagged `Result` as `{"Ok":…}` in NAPI invoke.
    ident.ends_with("Result").then_some(inner)
}

enum IntType {
    I64,
    I32,
    U64,
    U32,
    Usize,
}

fn integer_type(ty: &Type) -> Option<IntType> {
    let id = last_segment_ident(ty)?;
    match id.to_string().as_str() {
        "i64" => Some(IntType::I64),
        "i32" => Some(IntType::I32),
        "u64" => Some(IntType::U64),
        "u32" => Some(IntType::U32),
        "usize" => Some(IntType::Usize),
        _ => None,
    }
}

fn extract_state_inner(ty: &Type) -> proc_macro2::TokenStream {
    let Type::Path(tp) = ty else {
        return quote! { () };
    };
    let last = tp.path.segments.last().unwrap();
    let PathArguments::AngleBracketed(ab) = &last.arguments else {
        return quote! { () };
    };
    for arg in &ab.args {
        if let GenericArgument::Type(inner_ty) = arg {
            return quote! { #inner_ty };
        }
    }
    quote! { () }
}
