//! `SoapClient::__getFunctions()` and `__getTypes()`: the WSDL's
//! operations and types as PHP-like declarations (formats from the
//! ext/soap tests' expected output: "RetType name(Type $part, ...)",
//! "list(...)", "struct Name {\n type field;\n}", "Item Name[]",
//! "list Name {Item}", "Base Name {enum, ...}").

use super::encode::array_info;
use super::sdl::{Enc, Function, Kind, MKind, Model, Part, Sdl, TId, TRef};
use super::xmlout::NS_XSD;

fn tref_name(sdl: &Sdl, r: &Option<TRef>) -> String {
    match r {
        None => "anyType".into(),
        Some(TRef::Id(i)) => type_name(sdl, *i),
        Some(TRef::Q(ns, n)) => match sdl.resolve(&TRef::Q(ns.clone(), n.clone())) {
            Some(Enc::Builtin(b)) => b.name.to_string(),
            Some(Enc::Type(t)) => type_name(sdl, t),
            None => n.clone(),
        },
    }
}

fn type_name(sdl: &Sdl, t: TId) -> String {
    let ty = &sdl.types[t];
    if ty.kind == Kind::Element {
        let e = sdl.deref_element(t);
        let ed = &sdl.types[e];
        if let Some((ns, n)) = &ed.r#ref
            && ns == NS_XSD
            && n == "schema"
        {
            return "<anyXML>".into();
        }
        if ed.r#ref.is_some() && e == t {
            return "<anyXML>".into();
        }
        return tref_name(sdl, &ed.base);
    }
    ty.name.clone()
}

fn part_type(sdl: &Sdl, p: &Part) -> String {
    if let Some((ns, n)) = &p.element {
        return match sdl.element(ns, n) {
            Some(e) => type_name(sdl, e),
            None => n.clone(),
        };
    }
    tref_name(sdl, &p.typ)
}

/// One __getFunctions() line.
pub fn function(sdl: &Sdl, f: &Function) -> String {
    let ret = match &f.output {
        None => "void".to_string(),
        Some(o) => match o.parts.len() {
            0 => "void".into(),
            1 => part_type(sdl, &o.parts[0]),
            _ => {
                let items: Vec<String> = o
                    .parts
                    .iter()
                    .map(|p| format!("{} ${}", part_type(sdl, p), p.name))
                    .collect();
                format!("list({})", items.join(", "))
            }
        },
    };
    let params: Vec<String> = f
        .input
        .parts
        .iter()
        .map(|p| format!("{} ${}", part_type(sdl, p), p.name))
        .collect();
    format!("{} {}({})", ret, f.name, params.join(", "))
}

fn model_fields(sdl: &Sdl, m: &Model, out: &mut String, depth: u32) {
    if depth > 32 {
        return;
    }
    match m.kind {
        MKind::Element => {
            if let Some(e) = m.elem {
                let ed = &sdl.types[sdl.deref_element(e)];
                let name = match &sdl.types[e].r#ref {
                    Some((_, n)) => n.clone(),
                    None => ed.name.clone(),
                };
                out.push_str(&format!(" {} {};\n", type_name(sdl, e), name));
            }
        }
        MKind::Any => out.push_str(" <anyXML> any;\n"),
        MKind::Group => {
            if let Some(g) = m.group.as_ref().and_then(|q| sdl.group(q)) {
                model_fields(sdl, g, out, depth + 1);
            }
            for it in &m.items {
                model_fields(sdl, it, out, depth + 1);
            }
        }
        _ => {
            for it in &m.items {
                model_fields(sdl, it, out, depth + 1);
            }
        }
    }
}

fn struct_fields(sdl: &Sdl, t: TId, out: &mut String, depth: u32) {
    if depth > 16 {
        return;
    }
    let ty = &sdl.types[t];
    use super::sdl::Deriv;
    match ty.deriv {
        // A complexContent extension lists only its own fields (observed
        // on PHP 8.5; test bugs/bug28985.phpt).
        Deriv::SimpleExtension | Deriv::SimpleRestriction => {
            match ty.base.as_ref().and_then(|b| sdl.resolve(b)) {
                Some(Enc::Type(b)) if b != t && sdl.types[b].kind == Kind::Complex => {
                    struct_fields(sdl, b, out, depth + 1);
                }
                _ if ty.deriv != Deriv::ComplexExtension => {
                    out.push_str(&format!(" {} _;\n", tref_name(sdl, &ty.base)));
                }
                _ => {}
            }
        }
        _ => {}
    }
    if let Some(m) = &ty.model {
        model_fields(sdl, m, out, 0);
    }
    for a in sdl.attrs_of(t) {
        if a.name.is_empty() {
            continue;
        }
        out.push_str(&format!(" {} {};\n", tref_name(sdl, &a.typ), a.name));
    }
}

/// One __getTypes() entry.
pub fn type_decl(sdl: &Sdl, t: TId) -> String {
    let ty = &sdl.types[t];
    match ty.kind {
        Kind::Simple | Kind::Element => {
            let mut s = format!("{} {}", tref_name(sdl, &ty.base), ty.name);
            if !ty.enums.is_empty() {
                s.push_str(&format!(" {{{}}}", ty.enums.join(", ")));
            }
            s
        }
        Kind::List => format!(
            "list {} {{{}}}",
            ty.name,
            tref_name(sdl, &ty.list_item)
        ),
        Kind::Union => {
            let m: Vec<String> = ty
                .union
                .iter()
                .map(|r| tref_name(sdl, &Some(r.clone())))
                .collect();
            format!("union {} {{{}}}", ty.name, m.join(","))
        }
        Kind::Complex => {
            if let Some(info) = array_info(sdl, t) {
                let item = match &info.item {
                    Some((ns, n)) => tref_name(sdl, &Some(TRef::Q(ns.clone(), n.clone()))),
                    None => match info.elem {
                        Some(e) => type_name(sdl, e),
                        None => "anyType".into(),
                    },
                };
                let dims = sdl
                    .attrs_of(t)
                    .iter()
                    .find_map(|a| a.array_type.as_ref().map(|x| x.2.clone()))
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| "[]".into());
                return format!("{} {}{}", item, ty.name, dims);
            }
            let mut s = format!("struct {} {{\n", ty.name);
            struct_fields(sdl, t, &mut s, 0);
            s.push('}');
            s
        }
    }
}
