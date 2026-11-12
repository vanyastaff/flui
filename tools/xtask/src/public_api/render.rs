//! Signatures as text: rustdoc's JSON types, generics and bounds written back
//! as Rust, with every path the facade can name written as its facade path.
//!
//! The JSON is read untyped, like the rest of the loader: a shape this module
//! does not know renders as `?`, so a rustdoc format change shows up as a
//! visible change of the snapshot instead of a silent gap.

use std::fmt::Write as _;

use serde_json::Value as Json;

use crate::api_closure;

/// Where a path a signature names leads: the facade path when the facade
/// reaches the item, otherwise the path rustdoc records for it.
pub(super) trait Names {
    /// The path to write for the item `id` of the crate whose document holds
    /// `id`, or `None` when the document records nothing for it.
    fn name(&self, id: &str) -> Option<String>;
}

/// The std items written by their last segment: the prelude's, which a reader
/// knows without a path.
const PRELUDE: &[&str] = &[
    "Option",
    "Some",
    "None",
    "Result",
    "Ok",
    "Err",
    "Vec",
    "String",
    "Box",
    "ToString",
    "ToOwned",
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    "Copy",
    "Clone",
    "Drop",
    "Default",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "Into",
    "TryFrom",
    "TryInto",
    "AsRef",
    "AsMut",
    "Iterator",
    "IntoIterator",
    "Extend",
    "DoubleEndedIterator",
    "ExactSizeIterator",
];

/// The written form of a path rustdoc records as `segments` of crate `krate`.
pub(super) fn recorded(krate: &str, segments: &[String]) -> String {
    let std = matches!(krate, "core" | "alloc" | "std");
    match segments.last() {
        Some(last) if std && PRELUDE.contains(&last.as_str()) => last.clone(),
        _ => segments.join("::"),
    }
}

/// A type.
pub(super) fn ty(names: &dyn Names, value: &Json) -> String {
    let Some((kind, inner)) = value.as_object().and_then(|map| map.iter().next()) else {
        return "?".to_owned();
    };
    match kind.as_str() {
        "resolved_path" => path(names, inner),
        "dyn_trait" => {
            let mut parts: Vec<String> = inner["traits"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|poly| {
                    format!(
                        "{}{}",
                        higher_ranked(names, &poly["generic_params"]),
                        path(names, &poly["trait"])
                    )
                })
                .collect();
            if let Some(lifetime) = inner["lifetime"].as_str() {
                parts.push(lifetime.to_owned());
            }
            format!("dyn {}", parts.join(" + "))
        }
        "generic" | "primitive" => inner.as_str().unwrap_or("?").to_owned(),
        "function_pointer" => {
            let header = header(&inner["header"]);
            format!(
                "{}{header}fn({}){}",
                higher_ranked(names, &inner["generic_params"]),
                inputs(names, &inner["sig"]["inputs"], false),
                output(names, &inner["sig"]["output"])
            )
        }
        "tuple" => {
            let items: Vec<String> = inner
                .as_array()
                .into_iter()
                .flatten()
                .map(|item| ty(names, item))
                .collect();
            if items.len() == 1 {
                format!("({},)", items[0])
            } else {
                format!("({})", items.join(", "))
            }
        }
        "slice" => format!("[{}]", ty(names, inner)),
        "array" => format!(
            "[{}; {}]",
            ty(names, &inner["type"]),
            inner["len"].as_str().unwrap_or("?")
        ),
        "pat" => ty(names, &inner["type"]),
        "impl_trait" => format!("impl {}", bounds(names, inner)),
        "infer" => "_".to_owned(),
        "raw_pointer" => format!(
            "*{} {}",
            if inner["is_mutable"].as_bool() == Some(true) {
                "mut"
            } else {
                "const"
            },
            ty(names, &inner["type"])
        ),
        "borrowed_ref" => {
            let lifetime = inner["lifetime"]
                .as_str()
                .map_or_else(String::new, |lifetime| format!("{lifetime} "));
            let mutable = if inner["is_mutable"].as_bool() == Some(true) {
                "mut "
            } else {
                ""
            };
            format!("&{lifetime}{mutable}{}", ty(names, &inner["type"]))
        }
        "qualified_path" => {
            let self_type = ty(names, &inner["self_type"]);
            let name = inner["name"].as_str().unwrap_or("?");
            let args = generic_args(names, &inner["args"]);
            if inner["trait"].is_null() {
                format!("{self_type}::{name}{args}")
            } else {
                format!(
                    "<{self_type} as {}>::{name}{args}",
                    path(names, &inner["trait"])
                )
            }
        }
        _ => "?".to_owned(),
    }
}

/// A path with its generic arguments.
pub(super) fn path(names: &dyn Names, value: &Json) -> String {
    let written = value["path"].as_str().unwrap_or("?");
    let name = value
        .get("id")
        .filter(|id| !id.is_null())
        .and_then(|id| names.name(&api_closure::id_text(id)))
        .unwrap_or_else(|| written.to_owned());
    format!("{name}{}", generic_args(names, &value["args"]))
}

/// `<A, B, Item = C>`, `(A) -> B`, or nothing.
fn generic_args(names: &dyn Names, value: &Json) -> String {
    if let Some(angle) = value.get("angle_bracketed") {
        let mut parts: Vec<String> = angle["args"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|arg| generic_arg(names, arg))
            .collect();
        for constraint in angle["constraints"].as_array().into_iter().flatten() {
            let name = format!(
                "{}{}",
                constraint["name"].as_str().unwrap_or("?"),
                generic_args(names, &constraint["args"])
            );
            let binding = &constraint["binding"];
            if let Some(equality) = binding.get("equality") {
                parts.push(format!("{name} = {}", term(names, equality)));
            } else if let Some(constraint) = binding.get("constraint") {
                parts.push(format!("{name}: {}", bounds(names, constraint)));
            }
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("<{}>", parts.join(", "))
        }
    } else if let Some(parenthesized) = value.get("parenthesized") {
        format!(
            "({}){}",
            parenthesized["inputs"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|input| ty(names, input))
                .collect::<Vec<_>>()
                .join(", "),
            output(names, &parenthesized["output"])
        )
    } else if value.as_str() == Some("return_type_notation") {
        "(..)".to_owned()
    } else {
        String::new()
    }
}

fn generic_arg(names: &dyn Names, arg: &Json) -> String {
    if let Some(lifetime) = arg.get("lifetime") {
        lifetime.as_str().unwrap_or("?").to_owned()
    } else if let Some(value) = arg.get("type") {
        ty(names, value)
    } else if let Some(constant) = arg.get("const") {
        constant["expr"].as_str().unwrap_or("?").to_owned()
    } else if arg.as_str() == Some("infer") {
        "_".to_owned()
    } else {
        "?".to_owned()
    }
}

/// The right side of `Item = …`: a type or a constant.
fn term(names: &dyn Names, value: &Json) -> String {
    if let Some(value) = value.get("type") {
        ty(names, value)
    } else if let Some(constant) = value.get("constant") {
        constant["expr"].as_str().unwrap_or("?").to_owned()
    } else {
        "?".to_owned()
    }
}

/// `A + ?Sized + 'a`.
pub(super) fn bounds(names: &dyn Names, value: &Json) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|bound| {
            if let Some(trait_bound) = bound.get("trait_bound") {
                let modifier = match trait_bound["modifier"].as_str() {
                    Some("maybe") => "?",
                    Some("maybe_const") => "[const] ",
                    _ => "",
                };
                format!(
                    "{}{modifier}{}",
                    higher_ranked(names, &trait_bound["generic_params"]),
                    path(names, &trait_bound["trait"])
                )
            } else if let Some(lifetime) = bound.get("outlives") {
                lifetime.as_str().unwrap_or("?").to_owned()
            } else if let Some(captured) = bound.get("use") {
                let captured: Vec<String> = captured
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|arg| {
                        arg.get("lifetime")
                            .or_else(|| arg.get("param"))
                            .and_then(Json::as_str)
                            .unwrap_or("?")
                            .to_owned()
                    })
                    .collect();
                format!("use<{}>", captured.join(", "))
            } else {
                "?".to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// `for<'a> `, or nothing.
fn higher_ranked(names: &dyn Names, params: &Json) -> String {
    let list = params_list(names, params);
    if list.is_empty() {
        String::new()
    } else {
        format!("for<{list}> ")
    }
}

/// `<'a, T: Bound = Default, const N: usize>`, or nothing. A synthetic
/// parameter (an argument's `impl Trait`) is the argument's, not a name.
pub(super) fn params(names: &dyn Names, generics: &Json) -> String {
    let list = params_list(names, &generics["params"]);
    if list.is_empty() {
        String::new()
    } else {
        format!("<{list}>")
    }
}

fn params_list(names: &dyn Names, params: &Json) -> String {
    params
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|param| {
            let name = param["name"].as_str().unwrap_or("?");
            let kind = &param["kind"];
            if let Some(lifetime) = kind.get("lifetime") {
                let outlives: Vec<&str> = lifetime["outlives"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .collect();
                Some(if outlives.is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}: {}", outlives.join(" + "))
                })
            } else if let Some(parameter) = kind.get("type") {
                if parameter["is_synthetic"].as_bool() == Some(true) {
                    return None;
                }
                let mut text = name.to_owned();
                let bounds = bounds(names, &parameter["bounds"]);
                if !bounds.is_empty() {
                    write!(text, ": {bounds}").expect("BUG: writing to a String");
                }
                if !parameter["default"].is_null() {
                    write!(text, " = {}", ty(names, &parameter["default"]))
                        .expect("BUG: writing to a String");
                }
                Some(text)
            } else if let Some(constant) = kind.get("const") {
                let mut text = format!("const {name}: {}", ty(names, &constant["type"]));
                if let Some(default) = constant["default"].as_str() {
                    write!(text, " = {default}").expect("BUG: writing to a String");
                }
                Some(text)
            } else {
                Some("?".to_owned())
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// ` where A: B, 'a: 'b`, or nothing.
pub(super) fn where_clause(names: &dyn Names, generics: &Json) -> String {
    let predicates: Vec<String> = generics["where_predicates"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|predicate| {
            if let Some(bound) = predicate.get("bound_predicate") {
                format!(
                    "{}{}: {}",
                    higher_ranked(names, &bound["generic_params"]),
                    ty(names, &bound["type"]),
                    bounds(names, &bound["bounds"])
                )
            } else if let Some(lifetime) = predicate.get("lifetime_predicate") {
                let outlives: Vec<&str> = lifetime["outlives"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .collect();
                format!(
                    "{}: {}",
                    lifetime["lifetime"].as_str().unwrap_or("?"),
                    outlives.join(" + ")
                )
            } else if let Some(equality) = predicate.get("eq_predicate") {
                format!(
                    "{} = {}",
                    ty(names, &equality["lhs"]),
                    term(names, &equality["rhs"])
                )
            } else {
                "?".to_owned()
            }
        })
        .collect();
    if predicates.is_empty() {
        String::new()
    } else {
        format!(" where {}", predicates.join(", "))
    }
}

/// `const async unsafe extern "C" `, each part only when it applies.
pub(super) fn header(header: &Json) -> String {
    let mut text = String::new();
    for (key, word) in [
        ("is_const", "const "),
        ("is_async", "async "),
        ("is_unsafe", "unsafe "),
    ] {
        if header[key].as_bool() == Some(true) {
            text.push_str(word);
        }
    }
    let abi = &header["abi"];
    if let Some(abi) = abi.as_str() {
        if abi != "Rust" {
            write!(text, "extern \"{abi}\" ").expect("BUG: writing to a String");
        }
    } else if let Some((name, _)) = abi.as_object().and_then(|map| map.iter().next()) {
        write!(text, "extern \"{name}\" ").expect("BUG: writing to a String");
    }
    text
}

/// A function's parameters; `methods` writes `self`, `&self` and `&mut self`
/// in their short forms.
pub(super) fn inputs(names: &dyn Names, value: &Json, methods: bool) -> String {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|input| {
            let name = input[0].as_str().unwrap_or("_");
            let value = &input[1];
            if methods && name == "self" {
                if value.get("generic").and_then(Json::as_str) == Some("Self") {
                    return "self".to_owned();
                }
                if let Some(reference) = value.get("borrowed_ref")
                    && reference["type"].get("generic").and_then(Json::as_str) == Some("Self")
                {
                    let lifetime = reference["lifetime"]
                        .as_str()
                        .map_or_else(String::new, |lifetime| format!("{lifetime} "));
                    let mutable = if reference["is_mutable"].as_bool() == Some(true) {
                        "mut "
                    } else {
                        ""
                    };
                    return format!("&{lifetime}{mutable}self");
                }
            }
            format!("{name}: {}", ty(names, value))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// ` -> T`, or nothing for `()`.
pub(super) fn output(names: &dyn Names, value: &Json) -> String {
    if value.is_null() {
        String::new()
    } else {
        format!(" -> {}", ty(names, value))
    }
}

/// A function or method: `pub fn name<G>(args) -> R where …`, with `path`
/// as its name.
pub(super) fn function(names: &dyn Names, path: &str, function: &Json) -> String {
    let generics = &function["generics"];
    format!(
        "{}fn {path}{}({}){}{}",
        header(&function["header"]),
        params(names, generics),
        inputs(names, &function["sig"]["inputs"], true),
        output(names, &function["sig"]["output"]),
        where_clause(names, generics)
    )
}
