//! Declarations: functions, parameters, types, attributes, classes,
//! interfaces, traits, enums and their members (PHP manual: "Functions",
//! "Classes and Objects", "Enumerations", "Attributes"; RFCs "Property
//! hooks", "Asymmetric visibility v2", "DNF types").

use super::{PResult, Parser};
use crate::ast::*;
use crate::token::{T, Token};

/// What a modifier list applies to, for PHP's modifier checks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModTarget {
    Class,
    AnonClass,
    Member,
    Constant,
    Method,
    Property,
    Parameter,
    Hook,
}

fn modifier_flag(k: T) -> Option<u16> {
    Some(match k {
        T::Public => Modifiers::PUBLIC,
        T::Protected => Modifiers::PROTECTED,
        T::Private => Modifiers::PRIVATE,
        T::Static => Modifiers::STATIC,
        T::Abstract => Modifiers::ABSTRACT,
        T::Final => Modifiers::FINAL,
        T::Readonly => Modifiers::READONLY,
        T::PublicSet => Modifiers::PUBLIC_SET,
        T::ProtectedSet => Modifiers::PROTECTED_SET,
        T::PrivateSet => Modifiers::PRIVATE_SET,
        T::Var => Modifiers::VAR,
        _ => return None,
    })
}

fn modifier_name(f: u16) -> &'static str {
    match f {
        Modifiers::PUBLIC => "public",
        Modifiers::PROTECTED => "protected",
        Modifiers::PRIVATE => "private",
        Modifiers::STATIC => "static",
        Modifiers::ABSTRACT => "abstract",
        Modifiers::FINAL => "final",
        Modifiers::READONLY => "readonly",
        Modifiers::PUBLIC_SET => "public(set)",
        Modifiers::PROTECTED_SET => "protected(set)",
        Modifiers::PRIVATE_SET => "private(set)",
        _ => "var",
    }
}

pub(crate) fn is_type_start(k: T) -> bool {
    matches!(
        k,
        T::String
            | T::NameQualified
            | T::NameFullyQualified
            | T::NameRelative
            | T::Array
            | T::Callable
            | T::Question
            | T::LParen
    )
}

impl<'a> Parser<'a> {
    /// Adds a modifier with PHP's checks (observed on PHP 8.5: "Multiple
    /// access type modifiers are not allowed", "Multiple final modifiers
    /// are not allowed", ...).
    pub(crate) fn add_modifier(
        &self,
        mods: &mut Modifiers,
        t: &Token,
        target: ModTarget,
    ) -> PResult<()> {
        let f = modifier_flag(t.kind).unwrap_or(0);
        let sp = self.tok_span(t);
        if f & Modifiers::VISIBILITY != 0 && mods.0 & Modifiers::VISIBILITY != 0 {
            return Err(self.fatal("Multiple access type modifiers are not allowed", sp));
        }
        if f & Modifiers::SET_VISIBILITY != 0 && mods.0 & Modifiers::SET_VISIBILITY != 0 {
            return Err(self.fatal("Multiple access type modifiers are not allowed", sp));
        }
        if mods.0 & f != 0 {
            return Err(self.fatal(
                format!("Multiple {} modifiers are not allowed", modifier_name(f)),
                sp,
            ));
        }
        let bad = match target {
            ModTarget::Constant => {
                f & (Modifiers::STATIC
                    | Modifiers::ABSTRACT
                    | Modifiers::READONLY
                    | Modifiers::SET_VISIBILITY)
            }
            ModTarget::Method => f & (Modifiers::READONLY | Modifiers::SET_VISIBILITY),
            ModTarget::Parameter => f & (Modifiers::STATIC | Modifiers::ABSTRACT),
            ModTarget::Hook => f & !Modifiers::FINAL,
            ModTarget::AnonClass => f & (Modifiers::FINAL | Modifiers::ABSTRACT),
            _ => 0,
        };
        if bad != 0 {
            let what = match target {
                ModTarget::Constant => "class constant",
                ModTarget::Method => "method",
                ModTarget::Parameter => "parameter",
                ModTarget::Hook => "property hook",
                _ => "anonymous class",
            };
            return Err(self.fatal(
                format!("Cannot use the {} modifier on a {}", modifier_name(f), what)
                    .replace("a anonymous", "an anonymous"),
                sp,
            ));
        }
        if matches!(target, ModTarget::Class | ModTarget::AnonClass)
            && ((f == Modifiers::FINAL && mods.has(Modifiers::ABSTRACT))
                || (f == Modifiers::ABSTRACT && mods.has(Modifiers::FINAL)))
        {
            return Err(self.fatal("Cannot use the final modifier on an abstract class", sp));
        }
        mods.0 |= f;
        Ok(())
    }

    /// Final checks on a member's modifiers once its kind is known.
    fn check_member_modifiers(
        &self,
        mods: Modifiers,
        target: ModTarget,
        span: Span,
    ) -> PResult<()> {
        let bad = match target {
            ModTarget::Constant => {
                mods.0
                    & (Modifiers::STATIC
                        | Modifiers::ABSTRACT
                        | Modifiers::READONLY
                        | Modifiers::SET_VISIBILITY)
            }
            ModTarget::Method => mods.0 & (Modifiers::READONLY | Modifiers::SET_VISIBILITY),
            _ => 0,
        };
        if bad != 0 {
            // The first offending modifier in flag order.
            let f = 1u16 << bad.trailing_zeros();
            let what = if target == ModTarget::Constant {
                "class constant"
            } else {
                "method"
            };
            return Err(self.fatal(
                format!("Cannot use the {} modifier on a {}", modifier_name(f), what),
                span,
            ));
        }
        if target == ModTarget::Method
            && mods.has(Modifiers::ABSTRACT)
            && mods.has(Modifiers::FINAL)
        {
            return Err(self.fatal("Cannot use the final modifier on an abstract method", span));
        }
        if target == ModTarget::Property
            && mods.has(Modifiers::ABSTRACT)
            && mods.has(Modifiers::FINAL)
        {
            return Err(self.fatal(
                "Cannot use the final modifier on an abstract property",
                span,
            ));
        }
        Ok(())
    }

    // ----- attributes -----

    /// `#[A, B(1)] #[C]`: one or more attribute groups.
    #[inline(never)]
    pub(crate) fn attributes(&mut self) -> PResult<&'a [AttributeGroup<'a>]> {
        let mut groups = self.bvec();
        while self.at(T::Attribute) {
            let start = self.pos;
            self.bump();
            let mut attrs = self.bvec();
            loop {
                let astart = self.pos;
                let t = *self.peek();
                if !matches!(
                    t.kind,
                    T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative
                ) {
                    // After a comma only `]` may follow (observed on PHP 8.5).
                    let ex: &[T] = if attrs.is_empty() {
                        &[]
                    } else {
                        &[T::RBracket]
                    };
                    return Err(self.unexpected(ex));
                }
                self.bump();
                let name = self.name_from(&t);
                let (args, has_args) = if self.at(T::LParen) {
                    let pstart = self.pos;
                    match self.arguments()? {
                        Args::List(v) => (v, true),
                        Args::FirstClassCallable => {
                            // Test Zend/tests/first_class_callable/
                            // first_class_callable_assert2.phpt.
                            let sp = self.span_from(pstart);
                            self.compile_error("Cannot create Closure as attribute argument", sp);
                            (&[][..], true)
                        }
                    }
                } else {
                    (&[][..], false)
                };
                attrs.push(Attribute {
                    name,
                    args,
                    has_args,
                    span: self.span_from(astart),
                });
                if self.eat(T::Comma) {
                    if self.at(T::RBracket) {
                        break;
                    }
                    continue;
                }
                break;
            }
            self.expect1(T::RBracket)?;
            groups.push(AttributeGroup {
                attrs: attrs.into_bump_slice(),
                span: self.span_from(start),
            });
        }
        Ok(groups.into_bump_slice())
    }

    // ----- types -----

    fn simple_type(&mut self) -> PResult<Type<'a>> {
        let t = *self.peek();
        match t.kind {
            // `static` is a type only in return position (test
            // Zend/tests/type_declarations/static_type_property.phpt).
            T::Static if !self.allow_static_type => Err(self.unexpected(&[])),
            T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative => {
                self.bump();
                Ok(Type::Named(self.name_from(&t)))
            }
            T::Array => {
                self.bump();
                Ok(Type::Array(self.tok_span(&t)))
            }
            T::Callable => {
                self.bump();
                Ok(Type::Callable(self.tok_span(&t)))
            }
            T::Static => {
                self.bump();
                Ok(Type::Static(self.tok_span(&t)))
            }
            _ => Err(self.unexpected(&[])),
        }
    }

    /// A type declaration (`?T`, `A|B`, `A&B`, `(A&B)|C`).
    pub(crate) fn type_decl(&mut self) -> PResult<Type<'a>> {
        let start = self.pos;
        if self.eat(T::Question) {
            let inner = self.simple_type()?;
            return Ok(Type::Nullable(self.alloc(inner), self.span_from(start)));
        }
        let first = if self.at(T::LParen) {
            self.dnf_group()?
        } else {
            self.simple_type()?
        };
        if self.at(T::Pipe) {
            let mut types = vec![first];
            while self.eat(T::Pipe) {
                let ty = if self.at(T::LParen) {
                    self.dnf_group()?
                } else {
                    self.simple_type()?
                };
                types.push(ty);
            }
            return Ok(Type::Union(
                self.arena.alloc_slice_fill_iter(types),
                self.span_from(start),
            ));
        }
        if matches!(first, Type::Intersection(..)) {
            // A lone `(A&B)` group is not a type (observed on PHP 8.5:
            // "unexpected token ")", expecting "|"").
            return Err(self.unexpected(&[T::Pipe]));
        }
        if self.at(T::AmpNotFollowedByVarOrVararg) {
            let mut types = vec![first];
            while self.eat(T::AmpNotFollowedByVarOrVararg) {
                types.push(self.simple_type()?);
            }
            return Ok(Type::Intersection(
                self.arena.alloc_slice_fill_iter(types),
                self.span_from(start),
            ));
        }
        Ok(first)
    }

    /// `(A&B)` inside a DNF type.
    fn dnf_group(&mut self) -> PResult<Type<'a>> {
        let start = self.pos;
        self.expect1(T::LParen)?;
        let mut types = vec![self.simple_type()?];
        // Test Zend/tests/gh18026.phpt: `expecting token "&"`.
        self.expect1(T::AmpNotFollowedByVarOrVararg)?;
        types.push(self.simple_type()?);
        while self.eat(T::AmpNotFollowedByVarOrVararg) {
            types.push(self.simple_type()?);
        }
        self.expect1(T::RParen)?;
        Ok(Type::Intersection(
            self.arena.alloc_slice_fill_iter(types),
            self.span_from(start),
        ))
    }

    pub(crate) fn return_type(&mut self) -> PResult<Type<'a>> {
        self.allow_static_type = true;
        let r = self.type_decl();
        self.allow_static_type = false;
        r
    }

    // ----- parameters -----

    /// `( params )`, at `(`.
    #[inline(never)]
    pub(crate) fn parameter_list(&mut self) -> PResult<&'a [Param<'a>]> {
        self.expect1(T::LParen)?;
        let mut params = self.bvec();
        while !self.at(T::RParen) {
            params.push(self.parameter()?);
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect1(T::RParen)?;
        Ok(params.into_bump_slice())
    }

    fn parameter(&mut self) -> PResult<Param<'a>> {
        let start = self.pos;
        let attrs = if self.at(T::Attribute) {
            self.attributes()?
        } else {
            &[]
        };
        let mut modifiers = Modifiers::default();
        while let Some(_f) = modifier_flag(self.peek_kind()) {
            if self.at(T::Var) {
                break;
            }
            let t = self.bump();
            self.add_modifier(&mut modifiers, &t, ModTarget::Parameter)?;
        }
        let ty = if is_type_start(self.peek_kind()) {
            Some(self.type_decl()?)
        } else {
            None
        };
        let by_ref = self.eat(T::AmpFollowedByVarOrVararg);
        let variadic = self.eat(T::Ellipsis);
        let at = self.pos;
        let vt = self.expect1(T::Variable)?;
        let doc_comment = self.doc_comment_before(at);
        let name = Ident {
            name: &self.text(&vt)[1..],
            span: self.tok_span(&vt),
        };
        let default = if self.eat(T::Eq) {
            Some(self.expr()?)
        } else {
            None
        };
        let hooks = if self.at(T::LBrace) {
            self.property_hooks()?
        } else {
            &[]
        };
        Ok(Param {
            attrs,
            modifiers,
            ty,
            by_ref,
            variadic,
            name,
            default,
            hooks,
            doc_comment,
            span: self.span_from(start),
        })
    }

    // ----- functions -----

    /// `function [&] name(...) [: T] { ... }` at `function`.
    #[inline(never)]
    pub(crate) fn function_decl(
        &mut self,
        attrs: &'a [AttributeGroup<'a>],
    ) -> PResult<FunctionDecl<'a>> {
        // Reflection's start line is the `function` keyword's (observed on
        // PHP 8.5): the span starts there, after attributes.
        let start = self.pos;
        let doc_comment = self.doc_comment_before(start);
        self.expect1(T::Function)?;
        let by_ref = self.eat_amp();
        let t = *self.peek();
        if !matches!(t.kind, T::String | T::Readonly) {
            return Err(self.unexpected(&[T::LParen]));
        }
        self.bump();
        let name = self.ident_from(&t)?;
        if !self.at(T::LParen) {
            return Err(self.unexpected(&[T::LParen]));
        }
        let params = self.parameter_list()?;
        let ret = if self.eat(T::Colon) {
            Some(self.return_type()?)
        } else {
            None
        };
        if !self.at(T::LBrace) {
            return Err(self.unexpected(&[T::LBrace]));
        }
        let body = self.block_body()?;
        Ok(FunctionDecl {
            name,
            attrs,
            by_ref,
            params,
            ret,
            body,
            doc_comment,
            span: self.span_from(start),
        })
    }

    // ----- classes -----

    /// A class-like declaration statement, at its keyword (after
    /// attributes and modifiers).
    #[inline(never)]
    pub(crate) fn class_decl(
        &mut self,
        attrs: &'a [AttributeGroup<'a>],
        mods: Modifiers,
    ) -> PResult<ClassDecl<'a>> {
        // The span starts at the keyword (Reflection's start line, observed
        // on PHP 8.5).
        let start = self.pos;
        let doc_comment = self.doc_comment_before(start);
        let kw = self.bump();
        let kind = match kw.kind {
            T::Class => ClassKind::Class,
            T::Interface => ClassKind::Interface,
            T::Trait => ClassKind::Trait,
            _ => ClassKind::Enum,
        };
        let t = *self.peek();
        if t.kind != T::String && !(kind != ClassKind::Enum && t.kind == T::Enum) {
            return Err(self.unexpected(&[T::String]));
        }
        self.bump();
        let name = self.ident_from(&t)?;
        self.class_body_decl(start, kind, Some(name), attrs, mods, doc_comment)
    }

    /// A class name in `extends`, `implements`, `use`, `insteadof` and
    /// `catch`: `static` is accepted here and rejected by the compiler
    /// ("Cannot use "static" as class name, as it is reserved").
    pub(crate) fn class_name(&mut self) -> PResult<Name<'a>> {
        let t = *self.peek();
        match t.kind {
            T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative => {
                self.bump();
                Ok(self.name_from(&t))
            }
            T::Static => {
                self.bump();
                Ok(Name {
                    kind: NameKind::Unqualified,
                    name: self.text(&t),
                    span: self.tok_span(&t),
                })
            }
            _ => Err(self.unexpected(&[])),
        }
    }

    fn name_list(&mut self) -> PResult<&'a [Name<'a>]> {
        let mut v = vec![self.class_name()?];
        while self.eat(T::Comma) {
            v.push(self.class_name()?);
        }
        Ok(self.arena.alloc_slice_fill_iter(v))
    }

    /// Everything after the class name (or after `new class(...)`).
    #[inline(never)]
    pub(crate) fn class_body_decl(
        &mut self,
        start: usize,
        kind: ClassKind,
        name: Option<Ident<'a>>,
        attrs: &'a [AttributeGroup<'a>],
        mods: Modifiers,
        doc_comment: Option<&'a [u8]>,
    ) -> PResult<ClassDecl<'a>> {
        let mut extends = None;
        let mut implements: &'a [Name<'a>] = &[];
        let mut enum_backing = None;
        match kind {
            ClassKind::Class => {
                if self.eat(T::Extends) {
                    extends = Some(self.class_name()?);
                }
                if self.eat(T::Implements) {
                    implements = self.name_list()?;
                }
            }
            ClassKind::Interface => {
                if self.eat(T::Extends) {
                    implements = self.name_list()?;
                }
            }
            ClassKind::Enum => {
                if self.eat(T::Colon) {
                    enum_backing = Some(self.type_decl()?);
                }
                if self.eat(T::Implements) {
                    implements = self.name_list()?;
                }
            }
            ClassKind::Trait => {}
        }
        if !self.at(T::LBrace) {
            return Err(self.unexpected(&[T::LBrace]));
        }
        self.bump();
        let mut members = self.bvec();
        while !self.at(T::RBrace) {
            members.push(self.class_member()?);
        }
        self.bump();
        Ok(ClassDecl {
            kind,
            name,
            attrs,
            modifiers: mods,
            extends,
            implements,
            enum_backing,
            members: members.into_bump_slice(),
            doc_comment,
            span: self.span_from(start),
        })
    }

    #[inline(never)]
    fn class_member(&mut self) -> PResult<ClassMember<'a>> {
        let start = self.pos;
        if self.at(T::Use) {
            return Ok(ClassMember::TraitUse(self.trait_use()?));
        }
        let attrs = if self.at(T::Attribute) {
            self.attributes()?
        } else {
            &[]
        };
        if self.at(T::Case) {
            let doc_comment = self.doc_comment_before(self.pos);
            self.bump();
            let t = *self.peek();
            if !(t.kind == T::String || t.kind.is_semi_reserved()) {
                return Err(self.unexpected(&[]));
            }
            self.bump();
            let name = self.ident_from(&t)?;
            let value = if self.eat(T::Eq) {
                Some(self.expr()?)
            } else {
                None
            };
            self.expect1(T::Semicolon)?;
            return Ok(ClassMember::EnumCase(EnumCase {
                attrs,
                name,
                value,
                doc_comment,
                span: self.span_from(start),
            }));
        }
        let mut mods = Modifiers::default();
        let first = *self.peek();
        while modifier_flag(self.peek_kind()).is_some() {
            let t = self.bump();
            self.add_modifier(&mut mods, &t, ModTarget::Member)?;
        }
        match self.peek_kind() {
            T::Const => {
                self.bump();
                let sp = self.tok_span(&first);
                self.check_member_modifiers(mods, ModTarget::Constant, sp)?;
                // `const T NAME = ...` (typed, PHP 8.3) or `const NAME = ...`.
                let typed = {
                    let k0 = self.peek_kind();
                    let k1 = self.peek_at(1);
                    matches!(k0, T::Question | T::LParen)
                        || ((is_type_start(k0) || k0 == T::Static) && k1 != T::Eq)
                };
                // `static` is a valid constant type (test
                // typed_class_constants_type_success3.phpt).
                let ty = if typed {
                    self.allow_static_type = true;
                    let r = self.type_decl();
                    self.allow_static_type = false;
                    Some(r?)
                } else {
                    None
                };
                let mut items = self.bvec();
                loop {
                    let istart = self.pos;
                    let doc = self.doc_comment_before(istart);
                    let t = *self.peek();
                    if !(t.kind == T::String || t.kind.is_semi_reserved()) {
                        return Err(self.unexpected(&[]));
                    }
                    self.bump();
                    let name = self.ident_from(&t)?;
                    self.expect(T::Eq, &[])?;
                    let value = self.expr()?;
                    items.push(ConstItem {
                        name,
                        value,
                        doc_comment: doc,
                        span: self.span_from(istart),
                    });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect(T::Semicolon, &[T::Comma, T::Semicolon])?;
                let doc_comment = items[0].doc_comment;
                Ok(ClassMember::Const(ClassConstDecl {
                    attrs,
                    modifiers: mods,
                    ty,
                    items: items.into_bump_slice(),
                    doc_comment,
                    span: self.span_from(start),
                }))
            }
            T::Function => {
                let fstart = self.pos;
                let doc_comment = self.doc_comment_before(fstart);
                self.bump();
                let sp = self.tok_span(&first);
                self.check_member_modifiers(mods, ModTarget::Method, sp)?;
                let by_ref = self.eat_amp();
                let t = *self.peek();
                if !(t.kind == T::String || t.kind.is_semi_reserved()) {
                    return Err(self.unexpected(&[]));
                }
                self.bump();
                let name = self.ident_from(&t)?;
                if !self.at(T::LParen) {
                    return Err(self.unexpected(&[T::LParen]));
                }
                let params = self.parameter_list()?;
                let ret = if self.eat(T::Colon) {
                    Some(self.return_type()?)
                } else {
                    None
                };
                let body = if self.eat(T::Semicolon) {
                    None
                } else if self.at(T::LBrace) {
                    Some(self.block_body()?)
                } else {
                    return Err(self.unexpected(&[T::Semicolon, T::LBrace]));
                };
                Ok(ClassMember::Method(MethodDecl {
                    attrs,
                    modifiers: mods,
                    by_ref,
                    name,
                    params,
                    ret,
                    body,
                    doc_comment,
                    span: self.span_from(fstart),
                }))
            }
            k if mods.0 != 0 && (k == T::Variable || is_type_start(k)) => {
                let sp = self.tok_span(&first);
                self.check_member_modifiers(mods, ModTarget::Property, sp)?;
                let ty = if self.at(T::Variable) {
                    None
                } else {
                    Some(self.type_decl()?)
                };
                let mut props = self.bvec();
                loop {
                    let pstart = self.pos;
                    let doc = self.doc_comment_before(pstart);
                    let vt = self.expect1(T::Variable)?;
                    let name = Ident {
                        name: &self.text(&vt)[1..],
                        span: self.tok_span(&vt),
                    };
                    let default = if self.eat(T::Eq) {
                        Some(self.expr()?)
                    } else {
                        None
                    };
                    if self.at(T::LBrace) {
                        let hooks = self.property_hooks()?;
                        props.push(PropertyItem {
                            name,
                            default,
                            hooks,
                            has_hook_list: true,
                            doc_comment: doc,
                            span: self.span_from(pstart),
                        });
                        // A hooked property ends the declaration.
                        let doc_comment = props[0].doc_comment;
                        return Ok(ClassMember::Property(PropertyDecl {
                            attrs,
                            modifiers: mods,
                            ty,
                            props: props.into_bump_slice(),
                            doc_comment,
                            span: self.span_from(start),
                        }));
                    }
                    props.push(PropertyItem {
                        name,
                        default,
                        hooks: &[],
                        has_hook_list: false,
                        doc_comment: doc,
                        span: self.span_from(pstart),
                    });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect(T::Semicolon, &[T::Comma, T::Semicolon])?;
                let doc_comment = props[0].doc_comment;
                Ok(ClassMember::Property(PropertyDecl {
                    attrs,
                    modifiers: mods,
                    ty,
                    props: props.into_bump_slice(),
                    doc_comment,
                    span: self.span_from(start),
                }))
            }
            _ => {
                // Observed on PHP 8.5: `class A { catch }` expects
                // "function", `class A { public catch }` a variable.
                if mods.0 != 0 {
                    return Err(self.unexpected(&[T::Variable]));
                }
                Err(self.unexpected(&[T::Function]))
            }
        }
    }

    /// `{ get => ...; set { ... } }`
    #[inline(never)]
    fn property_hooks(&mut self) -> PResult<&'a [PropertyHook<'a>]> {
        self.expect1(T::LBrace)?;
        let mut hooks = self.bvec();
        while !self.at(T::RBrace) {
            let start = self.pos;
            let attrs = if self.at(T::Attribute) {
                self.attributes()?
            } else {
                &[]
            };
            let doc_comment = self.doc_comment_before(start);
            let mut modifiers = Modifiers::default();
            while modifier_flag(self.peek_kind()).is_some() {
                let t = self.bump();
                self.add_modifier(&mut modifiers, &t, ModTarget::Hook)?;
            }
            let by_ref = self.eat_amp();
            let t = *self.peek();
            if !(t.kind == T::String || t.kind.is_semi_reserved()) {
                return Err(self.unexpected(&[T::String]));
            }
            self.bump();
            let name = self.ident_from(&t)?;
            let params = if self.at(T::LParen) {
                Some(self.parameter_list()?)
            } else {
                None
            };
            let body = match self.peek_kind() {
                T::Semicolon => {
                    self.bump();
                    HookBody::Abstract
                }
                T::DoubleArrow => {
                    self.bump();
                    let e = self.expr()?;
                    self.expect(T::Semicolon, &[])?;
                    HookBody::Expr(self.alloc(e))
                }
                T::LBrace => HookBody::Block(self.block_body()?),
                _ => return Err(self.unexpected(&[T::Semicolon, T::LBrace, T::DoubleArrow])),
            };
            hooks.push(PropertyHook {
                attrs,
                modifiers,
                by_ref,
                name,
                params,
                body,
                doc_comment,
                span: self.span_from(start),
            });
        }
        self.bump();
        Ok(hooks.into_bump_slice())
    }

    /// `use A, B { ... }` inside a class.
    #[inline(never)]
    fn trait_use(&mut self) -> PResult<TraitUse<'a>> {
        let start = self.pos;
        self.bump();
        let traits = self.name_list()?;
        let mut adaptations = self.bvec();
        let mut has_block = false;
        if self.eat(T::LBrace) {
            has_block = true;
            while !self.at(T::RBrace) {
                adaptations.push(self.trait_adaptation()?);
            }
            self.bump();
        } else {
            self.expect(T::Semicolon, &[T::Comma, T::Semicolon, T::LBrace])?;
        }
        Ok(TraitUse {
            traits,
            adaptations: adaptations.into_bump_slice(),
            has_block,
            span: self.span_from(start),
        })
    }

    fn member_ident(&mut self) -> PResult<Ident<'a>> {
        let t = *self.peek();
        if !(t.kind == T::String || t.kind.is_semi_reserved()) {
            return Err(self.unexpected(&[T::String]));
        }
        self.bump();
        self.ident_from(&t)
    }

    fn trait_adaptation(&mut self) -> PResult<TraitAdaptation<'a>> {
        let start = self.pos;
        // `A::m` or `m`
        if !(self.peek_kind() == T::String || self.peek_kind().is_semi_reserved())
            && !matches!(
                self.peek_kind(),
                T::NameQualified | T::NameFullyQualified | T::NameRelative
            )
        {
            return Err(self.unexpected(&[]));
        }
        let method_kw = !matches!(
            self.peek_kind(),
            T::String | T::NameQualified | T::NameFullyQualified | T::NameRelative
        );
        let (trait_, method) = if matches!(
            self.peek_kind(),
            T::NameQualified | T::NameFullyQualified | T::NameRelative
        ) || (matches!(self.peek_kind(), T::String | T::Static)
            && self.peek_at(1) == T::DoubleColon)
        {
            let n = self.class_name()?;
            self.expect1(T::DoubleColon)?;
            (Some(n), self.member_ident()?)
        } else {
            (None, self.member_ident()?)
        };
        if self.at(T::Insteadof) {
            let Some(trait_) = trait_ else {
                return Err(self.unexpected(&[T::DoubleColon]));
            };
            self.bump();
            let insteadof = self.name_list()?;
            self.expect(T::Semicolon, &[T::Comma, T::Semicolon])?;
            return Ok(TraitAdaptation::Precedence {
                trait_,
                method,
                insteadof,
                span: self.span_from(start),
            });
        }
        if !self.at(T::As) {
            // Observed on PHP 8.5: `A::m }` expects "as", `m }` "::" (`m`
            // could be a trait name), a keyword method name "as".
            let ex: &[T] = if trait_.is_some() || method_kw {
                &[T::As]
            } else {
                &[T::DoubleColon]
            };
            return Err(self.unexpected(ex));
        }
        self.bump();
        let mut modifiers = Modifiers::default();
        let mut alias = None;
        let t = *self.peek();
        if let Some(f) = modifier_flag(t.kind) {
            if t.kind == T::Static || t.kind == T::Abstract {
                return Err(self.fatal(
                    format!(
                        "Cannot use \"{}\" as method modifier in trait alias",
                        modifier_name(f)
                    ),
                    self.tok_span(&t),
                ));
            }
            if t.kind == T::Readonly {
                // Test Zend/tests/readonly_props/readonly_method_trait.phpt.
                return Err(self.fatal(
                    "Cannot use the readonly modifier on a method",
                    self.tok_span(&t),
                ));
            }
            self.bump();
            modifiers.0 |= f;
            if self.at(T::String) || self.peek_kind().is_semi_reserved() {
                alias = Some(self.member_ident()?);
            }
        } else {
            let t = *self.peek();
            if !(t.kind == T::String || t.kind.is_semi_reserved()) {
                return Err(self.unexpected(&[]));
            }
            alias = Some(self.member_ident()?);
        }
        self.expect1(T::Semicolon)?;
        Ok(TraitAdaptation::Alias {
            trait_,
            method,
            modifiers,
            alias,
            span: self.span_from(start),
        })
    }

    /// Class modifiers before `class` at statement level.
    pub(crate) fn class_modifiers(&mut self) -> PResult<Modifiers> {
        let mut mods = Modifiers::default();
        while matches!(self.peek_kind(), T::Abstract | T::Final | T::Readonly) {
            let t = self.bump();
            self.add_modifier(&mut mods, &t, ModTarget::Class)?;
        }
        if !self.at(T::Class) {
            return Err(self.unexpected(&[T::Abstract, T::Final, T::Readonly, T::Class]));
        }
        Ok(mods)
    }
}
