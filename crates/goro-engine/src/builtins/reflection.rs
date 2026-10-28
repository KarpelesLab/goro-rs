//! Reflection (PHP manual: Reflection). Classes are declared from PHP
//! stubs written from the manual's synopses; their methods are native.

use std::rc::Rc;

use goro_runtime::{Key, ObjRef, PhpArray, PhpStr, Value};

use crate::bytecode::{
    AttrInfo, ConstExpr, FuncCode, ParamDefault, ParamInfo, TypeHint, fflags, tmask,
};
use crate::class::{Class, ClassKind, ConstState, DisplayName, TypeName, cflags, lower, mflags};
use crate::func::{Args, Func, FuncKind};
use crate::registry::{ConstVal, Module};
use crate::vm::calls::{Callee, ClosureData};
use crate::vm::{R, Unwind, Vm};

const STUBS: &str = r#"<?php
class ReflectionException extends Exception {}
final class Reflection { public static function getModifierNames(int $modifiers): array {} }
interface Reflector extends Stringable {}
abstract class ReflectionFunctionAbstract implements Reflector {
    public string $name = "";
    public function inNamespace(): bool {}
    public function isClosure(): bool {}
    public function isDeprecated(): bool {}
    public function isInternal(): bool {}
    public function isUserDefined(): bool {}
    public function isGenerator(): bool {}
    public function isVariadic(): bool {}
    public function isStatic(): bool {}
    public function getClosureThis(): ?object {}
    public function getClosureScopeClass(): ?ReflectionClass {}
    public function getClosureCalledClass(): ?ReflectionClass {}
    public function getClosureUsedVariables(): array {}
    public function getDocComment(): string|false {}
    public function getEndLine(): int|false {}
    public function getExtension(): ?ReflectionExtension {}
    public function getExtensionName(): string|false {}
    public function getFileName(): string|false {}
    public function getName(): string {}
    public function getNamespaceName(): string {}
    public function getNumberOfParameters(): int {}
    public function getNumberOfRequiredParameters(): int {}
    public function getParameters(): array {}
    public function getShortName(): string {}
    public function getStartLine(): int|false {}
    public function getStaticVariables(): array {}
    public function returnsReference(): bool {}
    public function hasReturnType(): bool {}
    public function getReturnType(): ?ReflectionType {}
    public function hasTentativeReturnType(): bool {}
    public function getTentativeReturnType(): ?ReflectionType {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
}
class ReflectionFunction extends ReflectionFunctionAbstract {
    const IS_DEPRECATED = 2048;
    public function __construct(Closure|string $function) {}
    public function __toString(): string {}
    public function isAnonymous(): bool {}
    public function isDisabled(): bool {}
    public function invoke(mixed ...$args): mixed {}
    public function invokeArgs(array $args = []): mixed {}
    public function getClosure(): Closure {}
}
final class ReflectionGenerator {
    public function __construct(Generator $generator) {}
    public function getExecutingLine(): int {}
    public function getExecutingFile(): string {}
    public function getTrace(int $options = DEBUG_BACKTRACE_PROVIDE_OBJECT): array {}
    public function getFunction(): ReflectionFunctionAbstract {}
    public function getThis(): ?object {}
    public function getExecutingGenerator(): Generator {}
    public function isClosed(): bool {}
}
class ReflectionMethod extends ReflectionFunctionAbstract {
    const IS_STATIC = 16;
    const IS_PUBLIC = 1;
    const IS_PROTECTED = 2;
    const IS_PRIVATE = 4;
    const IS_ABSTRACT = 64;
    const IS_FINAL = 32;
    public string $class = "";
    public function __construct(object|string $objectOrMethod, ?string $method = null) {}
    public static function createFromMethodName(string $method): static {}
    public function __toString(): string {}
    public function isPublic(): bool {}
    public function isPrivate(): bool {}
    public function isProtected(): bool {}
    public function isAbstract(): bool {}
    public function isFinal(): bool {}
    public function isConstructor(): bool {}
    public function isDestructor(): bool {}
    public function getClosure(?object $object = null): Closure {}
    public function getModifiers(): int {}
    public function invoke(?object $object, mixed ...$args): mixed {}
    public function invokeArgs(?object $object, array $args = []): mixed {}
    public function getDeclaringClass(): ReflectionClass {}
    public function getPrototype(): ReflectionMethod {}
    public function hasPrototype(): bool {}
    public function setAccessible(bool $accessible): void {}
}
class ReflectionClass implements Reflector {
    const IS_IMPLICIT_ABSTRACT = 16;
    const IS_EXPLICIT_ABSTRACT = 64;
    const IS_FINAL = 32;
    const IS_READONLY = 65536;
    const SKIP_INITIALIZATION_ON_SERIALIZE = 8;
    const SKIP_DESTRUCTOR = 16;
    public string $name = "";
    public function __construct(object|string $objectOrClass) {}
    public function newLazyGhost(callable $initializer, int $options = 0): object {}
    public function newLazyProxy(callable $factory, int $options = 0): object {}
    public function resetAsLazyGhost(object $object, callable $initializer, int $options = 0): void {}
    public function resetAsLazyProxy(object $object, callable $factory, int $options = 0): void {}
    public function initializeLazyObject(object $object): object {}
    public function isUninitializedLazyObject(object $object): bool {}
    public function markLazyObjectAsInitialized(object $object): object {}
    public function getLazyInitializer(object $object): ?callable {}
    public function __toString(): string {}
    public function getName(): string {}
    public function isInternal(): bool {}
    public function isUserDefined(): bool {}
    public function isAnonymous(): bool {}
    public function isInstantiable(): bool {}
    public function isCloneable(): bool {}
    public function getFileName(): string|false {}
    public function getStartLine(): int|false {}
    public function getEndLine(): int|false {}
    public function getDocComment(): string|false {}
    public function getConstructor(): ?ReflectionMethod {}
    public function hasMethod(string $name): bool {}
    public function getMethod(string $name): ReflectionMethod {}
    public function getMethods(?int $filter = null): array {}
    public function hasProperty(string $name): bool {}
    public function getProperty(string $name): ReflectionProperty {}
    public function getProperties(?int $filter = null): array {}
    public function hasConstant(string $name): bool {}
    public function getConstants(?int $filter = null): array {}
    public function getReflectionConstants(?int $filter = null): array {}
    public function getConstant(string $name): mixed {}
    public function getReflectionConstant(string $name): ReflectionClassConstant|false {}
    public function getInterfaces(): array {}
    public function getInterfaceNames(): array {}
    public function isInterface(): bool {}
    public function getTraits(): array {}
    public function getTraitNames(): array {}
    public function getTraitAliases(): array {}
    public function isTrait(): bool {}
    public function isEnum(): bool {}
    public function isAbstract(): bool {}
    public function isFinal(): bool {}
    public function isReadOnly(): bool {}
    public function getModifiers(): int {}
    public function isInstance(object $object): bool {}
    public function newInstance(mixed ...$args): object {}
    public function newInstanceWithoutConstructor(): object {}
    public function newInstanceArgs(array $args = []): ?object {}
    public function getParentClass(): ReflectionClass|false {}
    public function isSubclassOf(ReflectionClass|string $class): bool {}
    public function getStaticProperties(): array {}
    public function getStaticPropertyValue(string $name, mixed $default = UNKNOWN): mixed {}
    public function setStaticPropertyValue(string $name, mixed $value): void {}
    public function getDefaultProperties(): array {}
    public function isIterable(): bool {}
    public function isIterateable(): bool {}
    public function implementsInterface(ReflectionClass|string $interface): bool {}
    public function getExtension(): ?ReflectionExtension {}
    public function getExtensionName(): string|false {}
    public function inNamespace(): bool {}
    public function getNamespaceName(): string {}
    public function getShortName(): string {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
}
class ReflectionObject extends ReflectionClass {
    public function __construct(object $object) {}
}
enum PropertyHookType: string { case Get = 'get'; case Set = 'set'; }
class ReflectionProperty implements Reflector {
    const IS_STATIC = 16;
    const IS_READONLY = 128;
    const IS_PUBLIC = 1;
    const IS_PROTECTED = 2;
    const IS_PRIVATE = 4;
    const IS_ABSTRACT = 64;
    const IS_PROTECTED_SET = 2048;
    const IS_PRIVATE_SET = 4096;
    const IS_VIRTUAL = 512;
    const IS_FINAL = 32;
    public string $name = "";
    public string $class = "";
    public function __construct(object|string $class, string $property) {}
    public function __toString(): string {}
    public function getName(): string {}
    public function getMangledName(): string {}
    public function getValue(?object $object = null): mixed {}
    public function setValue(mixed $objectOrValue, mixed $value = UNKNOWN): void {}
    public function getRawValue(object $object): mixed {}
    public function setRawValue(object $object, mixed $value): void {}
    public function isInitialized(?object $object = null): bool {}
    public function isPublic(): bool {}
    public function isPrivate(): bool {}
    public function isProtected(): bool {}
    public function isPrivateSet(): bool {}
    public function isProtectedSet(): bool {}
    public function isStatic(): bool {}
    public function isReadOnly(): bool {}
    public function isDefault(): bool {}
    public function isDynamic(): bool {}
    public function isAbstract(): bool {}
    public function isVirtual(): bool {}
    public function isPromoted(): bool {}
    public function isFinal(): bool {}
    public function getModifiers(): int {}
    public function getDeclaringClass(): ReflectionClass {}
    public function getDocComment(): string|false {}
    public function setAccessible(bool $accessible): void {}
    public function getType(): ?ReflectionType {}
    public function getSettableType(): ?ReflectionType {}
    public function hasType(): bool {}
    public function hasDefaultValue(): bool {}
    public function getDefaultValue(): mixed {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
    public function hasHooks(): bool {}
    public function getHooks(): array {}
    public function hasHook(PropertyHookType $type): bool {}
    public function getHook(PropertyHookType $type): ?ReflectionMethod {}
    public function setRawValueWithoutLazyInitialization(object $object, mixed $value): void {}
    public function skipLazyInitialization(object $object): void {}
    public function isLazy(object $object): bool {}
}
class ReflectionClassConstant implements Reflector {
    const IS_PUBLIC = 1;
    const IS_PROTECTED = 2;
    const IS_PRIVATE = 4;
    const IS_FINAL = 32;
    public string $name = "";
    public string $class = "";
    public function __construct(object|string $class, string $constant) {}
    public function __toString(): string {}
    public function getName(): string {}
    public function getValue(): mixed {}
    public function isPublic(): bool {}
    public function isPrivate(): bool {}
    public function isProtected(): bool {}
    public function isFinal(): bool {}
    public function getModifiers(): int {}
    public function getDeclaringClass(): ReflectionClass {}
    public function getDocComment(): string|false {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
    public function isEnumCase(): bool {}
    public function isDeprecated(): bool {}
    public function hasType(): bool {}
    public function getType(): ?ReflectionType {}
}
class ReflectionParameter implements Reflector {
    public string $name = "";
    public function __construct($function, int|string $param) {}
    public function __toString(): string {}
    public function getName(): string {}
    public function isPassedByReference(): bool {}
    public function canBePassedByValue(): bool {}
    public function getDeclaringFunction(): ReflectionFunctionAbstract {}
    public function getDeclaringClass(): ?ReflectionClass {}
    public function getClass(): ?ReflectionClass {}
    public function hasType(): bool {}
    public function getType(): ?ReflectionType {}
    public function isArray(): bool {}
    public function isCallable(): bool {}
    public function allowsNull(): bool {}
    public function getPosition(): int {}
    public function isOptional(): bool {}
    public function isDefaultValueAvailable(): bool {}
    public function getDefaultValue(): mixed {}
    public function isDefaultValueConstant(): bool {}
    public function getDefaultValueConstantName(): ?string {}
    public function isVariadic(): bool {}
    public function isPromoted(): bool {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
}
abstract class ReflectionType implements Stringable {
    public function allowsNull(): bool {}
    public function __toString(): string {}
}
class ReflectionNamedType extends ReflectionType {
    public function getName(): string {}
    public function isBuiltin(): bool {}
}
class ReflectionUnionType extends ReflectionType {
    public function getTypes(): array {}
}
class ReflectionIntersectionType extends ReflectionType {
    public function getTypes(): array {}
}
class ReflectionAttribute implements Reflector {
    const IS_INSTANCEOF = 2;
    public function getName(): string {}
    public function getTarget(): int {}
    public function isRepeated(): bool {}
    public function getArguments(): array {}
    public function newInstance(): object {}
    public function __toString(): string {}
    private function __construct() {}
}
class ReflectionEnum extends ReflectionClass {
    public function __construct(object|string $objectOrClass) {}
    public function hasCase(string $name): bool {}
    public function getCase(string $name): ReflectionEnumUnitCase {}
    public function getCases(): array {}
    public function isBacked(): bool {}
    public function getBackingType(): ?ReflectionNamedType {}
}
class ReflectionEnumUnitCase extends ReflectionClassConstant {
    public function __construct(object|string $class, string $constant) {}
    public function getEnum(): ReflectionEnum {}
    public function getValue(): UnitEnum {}
}
class ReflectionEnumBackedCase extends ReflectionEnumUnitCase {
    public function __construct(object|string $class, string $constant) {}
    public function getBackingValue(): int|string {}
}
final class ReflectionReference {
    public static function fromArrayElement(array $array, int|string $key): ?ReflectionReference {}
    public function getId(): string {}
    private function __construct() {}
}
class ReflectionExtension implements Reflector {
    public string $name = "";
    public function __construct(string $name) {}
    public function __toString(): string {}
    public function getName(): string {}
    public function getVersion(): ?string {}
    public function getFunctions(): array {}
    public function getConstants(): array {}
    public function getINIEntries(): array {}
    public function getClasses(): array {}
    public function getClassNames(): array {}
    public function getDependencies(): array {}
    public function info(): void {}
    public function isPersistent(): bool {}
    public function isTemporary(): bool {}
}
final class ReflectionConstant implements Reflector {
    public string $name = "";
    public function __construct(string $name) {}
    public function getName(): string {}
    public function getNamespaceName(): string {}
    public function getShortName(): string {}
    public function getValue(): mixed {}
    public function isDeprecated(): bool {}
    public function getFileName(): string|false {}
    public function getExtension(): ?ReflectionExtension {}
    public function getExtensionName(): string|false {}
    public function getAttributes(?string $name = null, int $flags = 0): array {}
    public function __toString(): string {}
}
final class ReflectionFiber {
    public function __construct(Fiber $fiber) {}
    public function getFiber(): Fiber {}
    public function getExecutingFile(): string {}
    public function getExecutingLine(): int {}
    public function getCallable(): callable {}
    public function getTrace(int $options = DEBUG_BACKTRACE_PROVIDE_OBJECT): array {}
}
"#;

macro_rules! cls {
    ($l:literal) => {
        ($l, STUBS)
    };
}

pub static MODULE: Module = Module {
    name: "Reflection",
    classes: &[
        cls!("reflectionexception"),
        cls!("reflection"),
        cls!("reflector"),
        cls!("reflectionfunctionabstract"),
        cls!("reflectionfunction"),
        cls!("reflectiongenerator"),
        cls!("reflectionmethod"),
        cls!("reflectionclass"),
        cls!("reflectionobject"),
        cls!("propertyhooktype"),
        cls!("reflectionproperty"),
        cls!("reflectionclassconstant"),
        cls!("reflectionparameter"),
        cls!("reflectiontype"),
        cls!("reflectionnamedtype"),
        cls!("reflectionuniontype"),
        cls!("reflectionintersectiontype"),
        cls!("reflectionattribute"),
        cls!("reflectionenum"),
        cls!("reflectionenumunitcase"),
        cls!("reflectionenumbackedcase"),
        cls!("reflectionreference"),
        cls!("reflectionextension"),
        cls!("reflectionconstant"),
        cls!("reflectionfiber"),
    ],
    methods: &[
        ("reflection::getmodifiernames", modifier_names),
        // functions and methods
        ("reflectionfunctionabstract::innamespace", fa_in_namespace),
        ("reflectionfunctionabstract::isclosure", fa_is_closure),
        ("reflectionfunctionabstract::isdeprecated", fa_is_deprecated),
        ("reflectionfunctionabstract::isinternal", fa_is_internal),
        ("reflectionfunctionabstract::isuserdefined", fa_is_user),
        ("reflectionfunctionabstract::isgenerator", fa_is_generator),
        ("reflectionfunctionabstract::isvariadic", fa_is_variadic),
        ("reflectionfunctionabstract::isstatic", fa_is_static),
        (
            "reflectionfunctionabstract::getclosurethis",
            fa_closure_this,
        ),
        (
            "reflectionfunctionabstract::getclosurescopeclass",
            fa_closure_scope,
        ),
        (
            "reflectionfunctionabstract::getclosurecalledclass",
            fa_closure_called,
        ),
        (
            "reflectionfunctionabstract::getclosureusedvariables",
            fa_closure_used,
        ),
        ("reflectionfunctionabstract::getdoccomment", fa_doc),
        ("reflectionfunctionabstract::getendline", fa_end_line),
        ("reflectionfunctionabstract::getextension", fa_extension),
        (
            "reflectionfunctionabstract::getextensionname",
            fa_extension_name,
        ),
        ("reflectionfunctionabstract::getfilename", fa_file),
        ("reflectionfunctionabstract::getname", fa_name),
        ("reflectionfunctionabstract::getnamespacename", fa_ns),
        (
            "reflectionfunctionabstract::getnumberofparameters",
            fa_nparams,
        ),
        (
            "reflectionfunctionabstract::getnumberofrequiredparameters",
            fa_nreq,
        ),
        ("reflectionfunctionabstract::getparameters", fa_params),
        ("reflectionfunctionabstract::getshortname", fa_short),
        ("reflectionfunctionabstract::getstartline", fa_start_line),
        ("reflectionfunctionabstract::getstaticvariables", fa_statics),
        ("reflectionfunctionabstract::returnsreference", fa_by_ref),
        ("reflectionfunctionabstract::hasreturntype", fa_has_ret),
        ("reflectionfunctionabstract::getreturntype", fa_ret),
        (
            "reflectionfunctionabstract::hastentativereturntype",
            ret_false,
        ),
        (
            "reflectionfunctionabstract::gettentativereturntype",
            ret_null,
        ),
        ("reflectionfunctionabstract::getattributes", fa_attrs),
        ("reflectionfunction::__construct", f_construct),
        ("reflectionfunction::__tostring", f_to_string),
        ("reflectionfunction::isanonymous", f_is_anon),
        ("reflectionfunction::isdisabled", ret_false),
        ("reflectionfunction::invoke", f_invoke),
        ("reflectionfunction::invokeargs", f_invoke_args),
        ("reflectionfunction::getclosure", f_get_closure),
        ("reflectionmethod::__construct", m_construct),
        ("reflectionmethod::createfrommethodname", m_create_from),
        ("reflectionmethod::__tostring", f_to_string),
        ("reflectionmethod::ispublic", m_is_public),
        ("reflectionmethod::isprivate", m_is_private),
        ("reflectionmethod::isprotected", m_is_protected),
        ("reflectionmethod::isabstract", m_is_abstract),
        ("reflectionmethod::isfinal", m_is_final),
        ("reflectionmethod::isconstructor", m_is_ctor),
        ("reflectionmethod::isdestructor", m_is_dtor),
        ("reflectionmethod::getclosure", m_get_closure),
        ("reflectionmethod::getmodifiers", m_modifiers),
        ("reflectionmethod::invoke", m_invoke),
        ("reflectionmethod::invokeargs", m_invoke_args),
        ("reflectionmethod::getdeclaringclass", m_declaring),
        ("reflectionmethod::getprototype", m_prototype),
        ("reflectionmethod::hasprototype", m_has_prototype),
        ("reflectionmethod::setaccessible", noop),
        // classes
        ("reflectionclass::__construct", c_construct),
        ("reflectionobject::__construct", c_construct),
        ("reflectionclass::__tostring", c_to_string),
        ("reflectionclass::getname", c_name),
        ("reflectionclass::isinternal", c_is_internal),
        ("reflectionclass::isuserdefined", c_is_user),
        ("reflectionclass::isanonymous", c_is_anon),
        ("reflectionclass::isinstantiable", c_is_instantiable),
        ("reflectionclass::iscloneable", c_is_cloneable),
        ("reflectionclass::getfilename", c_file),
        ("reflectionclass::getstartline", c_start),
        ("reflectionclass::getendline", c_end),
        ("reflectionclass::getdoccomment", c_doc),
        ("reflectionclass::getconstructor", c_ctor),
        ("reflectionclass::hasmethod", c_has_method),
        ("reflectionclass::getmethod", c_get_method),
        ("reflectionclass::getmethods", c_methods),
        ("reflectionclass::hasproperty", c_has_prop),
        ("reflectionclass::getproperty", c_get_prop),
        ("reflectionclass::getproperties", c_props),
        ("reflectionclass::hasconstant", c_has_const),
        ("reflectionclass::getconstants", c_consts),
        ("reflectionclass::getreflectionconstants", c_refl_consts),
        ("reflectionclass::getconstant", c_const),
        ("reflectionclass::getreflectionconstant", c_refl_const),
        ("reflectionclass::getinterfaces", c_interfaces),
        ("reflectionclass::getinterfacenames", c_interface_names),
        ("reflectionclass::isinterface", c_is_interface),
        ("reflectionclass::gettraits", c_traits),
        ("reflectionclass::gettraitnames", c_trait_names),
        ("reflectionclass::gettraitaliases", c_trait_aliases),
        ("reflectionclass::istrait", c_is_trait),
        ("reflectionclass::isenum", c_is_enum),
        ("reflectionclass::isabstract", c_is_abstract),
        ("reflectionclass::isfinal", c_is_final),
        ("reflectionclass::isreadonly", c_is_readonly),
        ("reflectionclass::getmodifiers", c_modifiers),
        ("reflectionclass::isinstance", c_is_instance),
        ("reflectionclass::newinstance", c_new),
        (
            "reflectionclass::newinstancewithoutconstructor",
            c_new_no_ctor,
        ),
        ("reflectionclass::newinstanceargs", c_new_args),
        ("reflectionclass::getparentclass", c_parent),
        ("reflectionclass::issubclassof", c_is_subclass),
        ("reflectionclass::getstaticproperties", c_static_props),
        ("reflectionclass::getstaticpropertyvalue", c_get_static),
        ("reflectionclass::setstaticpropertyvalue", c_set_static),
        ("reflectionclass::getdefaultproperties", c_default_props),
        ("reflectionclass::isiterable", c_is_iterable),
        ("reflectionclass::isiterateable", c_is_iterable),
        ("reflectionclass::implementsinterface", c_implements),
        ("reflectionclass::getextension", c_extension),
        ("reflectionclass::getextensionname", c_extension_name),
        ("reflectionclass::innamespace", c_in_ns),
        ("reflectionclass::getnamespacename", c_ns),
        ("reflectionclass::getshortname", c_short),
        ("reflectionclass::getattributes", c_attrs),
        ("reflectionclass::newlazyghost", c_new_lazy_ghost),
        ("reflectionclass::newlazyproxy", c_new_lazy_proxy),
        ("reflectionclass::resetaslazyghost", c_reset_lazy_ghost),
        ("reflectionclass::resetaslazyproxy", c_reset_lazy_proxy),
        ("reflectionclass::initializelazyobject", c_init_lazy),
        ("reflectionclass::isuninitializedlazyobject", c_is_uninit_lazy),
        ("reflectionclass::marklazyobjectasinitialized", c_mark_lazy_init),
        ("reflectionclass::getlazyinitializer", c_get_lazy_init),
        // properties
        ("reflectionproperty::__construct", p_construct),
        ("reflectionproperty::__tostring", p_to_string),
        ("reflectionproperty::getname", p_name),
        ("reflectionproperty::getmangledname", p_mangled),
        ("reflectionproperty::getvalue", p_get_value),
        ("reflectionproperty::setvalue", p_set_value),
        ("reflectionproperty::getrawvalue", p_get_raw),
        ("reflectionproperty::setrawvalue", p_set_raw),
        ("reflectionproperty::isinitialized", p_is_init),
        ("reflectionproperty::ispublic", p_is_public),
        ("reflectionproperty::isprivate", p_is_private),
        ("reflectionproperty::isprotected", p_is_protected),
        ("reflectionproperty::isprivateset", p_is_private_set),
        ("reflectionproperty::isprotectedset", p_is_protected_set),
        ("reflectionproperty::isstatic", p_is_static),
        ("reflectionproperty::isreadonly", p_is_readonly),
        ("reflectionproperty::isdefault", p_is_default),
        ("reflectionproperty::isdynamic", p_is_dynamic),
        ("reflectionproperty::isabstract", p_is_abstract),
        ("reflectionproperty::isvirtual", p_is_virtual),
        ("reflectionproperty::ispromoted", p_is_promoted),
        ("reflectionproperty::isfinal", p_is_final),
        ("reflectionproperty::getmodifiers", p_modifiers),
        ("reflectionproperty::getdeclaringclass", p_declaring),
        ("reflectionproperty::getdoccomment", p_doc),
        ("reflectionproperty::setaccessible", noop),
        ("reflectionproperty::gettype", p_type),
        ("reflectionproperty::getsettabletype", p_type),
        ("reflectionproperty::hastype", p_has_type),
        ("reflectionproperty::hasdefaultvalue", p_has_default),
        ("reflectionproperty::getdefaultvalue", p_default),
        ("reflectionproperty::getattributes", p_attrs),
        ("reflectionproperty::hashooks", p_has_hooks),
        ("reflectionproperty::gethooks", p_hooks),
        ("reflectionproperty::hashook", p_has_hook),
        ("reflectionproperty::gethook", p_hook),
        ("reflectionproperty::setrawvaluewithoutlazyinitialization", p_set_raw_lazy),
        ("reflectionproperty::skiplazyinitialization", p_skip_lazy),
        ("reflectionproperty::islazy", p_is_lazy),
        // class constants
        ("reflectionclassconstant::__construct", cc_construct),
        ("reflectionenumunitcase::__construct", cc_construct),
        ("reflectionenumbackedcase::__construct", cc_construct),
        ("reflectionclassconstant::__tostring", cc_to_string),
        ("reflectionclassconstant::getname", cc_name),
        ("reflectionclassconstant::getvalue", cc_value),
        ("reflectionclassconstant::ispublic", cc_is_public),
        ("reflectionclassconstant::isprivate", cc_is_private),
        ("reflectionclassconstant::isprotected", cc_is_protected),
        ("reflectionclassconstant::isfinal", cc_is_final),
        ("reflectionclassconstant::getmodifiers", cc_modifiers),
        ("reflectionclassconstant::getdeclaringclass", cc_declaring),
        ("reflectionclassconstant::getdoccomment", cc_doc),
        ("reflectionclassconstant::getattributes", cc_attrs),
        ("reflectionclassconstant::isenumcase", cc_is_case),
        ("reflectionclassconstant::isdeprecated", cc_is_deprecated),
        ("reflectionclassconstant::hastype", cc_has_type),
        ("reflectionclassconstant::gettype", cc_type),
        // parameters
        ("reflectionparameter::__construct", par_construct),
        ("reflectionparameter::__tostring", par_to_string),
        ("reflectionparameter::getname", par_name),
        ("reflectionparameter::ispassedbyreference", par_by_ref),
        ("reflectionparameter::canbepassedbyvalue", par_by_value),
        ("reflectionparameter::getdeclaringfunction", par_func),
        ("reflectionparameter::getdeclaringclass", par_class),
        ("reflectionparameter::getclass", par_get_class),
        ("reflectionparameter::hastype", par_has_type),
        ("reflectionparameter::gettype", par_type),
        ("reflectionparameter::isarray", par_is_array),
        ("reflectionparameter::iscallable", par_is_callable),
        ("reflectionparameter::allowsnull", par_allows_null),
        ("reflectionparameter::getposition", par_position),
        ("reflectionparameter::isoptional", par_optional),
        (
            "reflectionparameter::isdefaultvalueavailable",
            par_has_default,
        ),
        ("reflectionparameter::getdefaultvalue", par_default),
        (
            "reflectionparameter::isdefaultvalueconstant",
            par_default_is_const,
        ),
        (
            "reflectionparameter::getdefaultvalueconstantname",
            par_default_const_name,
        ),
        ("reflectionparameter::isvariadic", par_variadic),
        ("reflectionparameter::ispromoted", par_promoted),
        ("reflectionparameter::getattributes", par_attrs),
        // types
        ("reflectiontype::allowsnull", t_allows_null),
        ("reflectiontype::__tostring", t_to_string),
        ("reflectionnamedtype::getname", t_name),
        ("reflectionnamedtype::isbuiltin", t_builtin),
        ("reflectionuniontype::gettypes", t_types),
        ("reflectionintersectiontype::gettypes", t_types),
        // attributes
        ("reflectionattribute::getname", a_name),
        ("reflectionattribute::gettarget", a_target),
        ("reflectionattribute::isrepeated", a_repeated),
        ("reflectionattribute::getarguments", a_args),
        ("reflectionattribute::newinstance", a_new),
        ("reflectionattribute::__tostring", a_to_string),
        // enums
        ("reflectionenum::__construct", e_construct),
        ("reflectionenum::hascase", e_has_case),
        ("reflectionenum::getcase", e_get_case),
        ("reflectionenum::getcases", e_cases),
        ("reflectionenum::isbacked", e_is_backed),
        ("reflectionenum::getbackingtype", e_backing_type),
        ("reflectionenumunitcase::getenum", ec_enum),
        ("reflectionenumunitcase::getvalue", cc_value),
        ("reflectionenumbackedcase::getbackingvalue", ec_backing),
        // misc
        ("reflectionreference::fromarrayelement", rr_from),
        ("reflectionreference::getid", rr_id),
        ("reflectionextension::__construct", ext_construct),
        ("reflectionextension::getname", ext_name),
        ("reflectionextension::getversion", ext_version),
        ("reflectionextension::getfunctions", ext_functions),
        ("reflectionextension::getconstants", ext_constants),
        ("reflectionextension::getinientries", ext_ini),
        ("reflectionextension::getclasses", ext_classes),
        ("reflectionextension::getclassnames", ext_class_names),
        ("reflectionextension::getdependencies", empty_array),
        ("reflectionextension::info", noop),
        ("reflectionextension::ispersistent", ret_true),
        ("reflectionextension::istemporary", ret_false),
        ("reflectionextension::__tostring", ext_name),
        ("reflectionconstant::__construct", gc_construct),
        ("reflectionconstant::getname", gc_name),
        ("reflectionconstant::getnamespacename", gc_ns),
        ("reflectionconstant::getshortname", gc_short),
        ("reflectionconstant::getvalue", gc_value),
        ("reflectionconstant::isdeprecated", ret_false),
        ("reflectionconstant::getfilename", ret_false),
        ("reflectionconstant::getextension", ret_null),
        ("reflectionconstant::getextensionname", ret_false),
        ("reflectionconstant::getattributes", empty_array),
        ("reflectionconstant::__tostring", gc_to_string),
        ("reflectiongenerator::__construct", g_construct),
        ("reflectiongenerator::getexecutingline", g_line),
        ("reflectiongenerator::getexecutingfile", g_file),
        ("reflectiongenerator::gettrace", empty_array),
        ("reflectiongenerator::getfunction", g_function),
        ("reflectiongenerator::getthis", g_this),
        ("reflectiongenerator::getexecutinggenerator", g_gen),
        ("reflectiongenerator::isclosed", g_closed),
    ],
    constants: &[],
    listed: true,
    ..Module::EMPTY
};

#[allow(dead_code)]
const _C: ConstVal = ConstVal::Null;

/// What a Reflection object reflects.
pub enum Refl {
    None,
    Class(Rc<Class>),
    /// ReflectionObject: the class and the object (dynamic properties).
    Object(Rc<Class>, ObjRef),
    Func {
        func: Rc<Func>,
        closure: Option<ObjRef>,
    },
    Method {
        func: Rc<Func>,
        cls: Rc<Class>,
    },
    Param {
        func: Rc<Func>,
        closure: Option<ObjRef>,
        idx: usize,
    },
    Prop {
        cls: Rc<Class>,
        name: PhpStr,
        dynamic: bool,
    },
    Const {
        cls: Rc<Class>,
        name: PhpStr,
    },
    Type {
        ty: TypeHint,
        single: Option<(PhpStr, bool)>,
        scope: *const Class,
    },
    Attr {
        info: AttrInfo,
        target: i64,
        scope: *const Class,
        repeated: bool,
    },
    Generator(ObjRef),
    Reference(usize),
    Extension(PhpStr),
    GlobalConst(PhpStr),
}

pub struct ReflData(pub Refl);

impl goro_runtime::NativeData for ReflData {
    fn trace(&self, visit: &mut dyn FnMut(&Value)) {
        match &self.0 {
            Refl::Func {
                closure: Some(c), ..
            }
            | Refl::Param {
                closure: Some(c), ..
            } => visit(&Value::Object(c.clone())),
            Refl::Generator(g) | Refl::Object(_, g) => visit(&Value::Object(g.clone())),
            _ => {}
        }
    }
}

fn data<T>(o: &ObjRef, f: impl FnOnce(&Refl) -> T) -> T {
    let n = o.obj().native.borrow();
    match n.as_ref().and_then(|n| {
        let any: &dyn std::any::Any = &**n;
        any.downcast_ref::<ReflData>()
    }) {
        Some(d) => f(&d.0),
        None => f(&Refl::None),
    }
}

fn set_data(o: &ObjRef, r: Refl) {
    *o.obj().native.borrow_mut() = Some(Box::new(ReflData(r)));
}

fn refl_error<T>(vm: &mut Vm, msg: &str) -> R<T> {
    vm.throw("ReflectionException", msg)
}

/// A new Reflection object of `class` reflecting `r`.
fn new_refl(vm: &mut Vm, class: &str, r: Refl) -> R<ObjRef> {
    let cls = vm
        .find_class(class.as_bytes(), false)?
        .expect("reflection class");
    let o = vm.instantiate_raw(&cls)?;
    match &r {
        Refl::Class(c) => vm.set_prop_raw(&o, b"name", Value::Str(c.name.clone())),
        Refl::Func { func, closure } => {
            let n = func_name(func, closure.is_some());
            vm.set_prop_raw(&o, b"name", Value::Str(n));
        }
        Refl::Method { func, .. } => {
            vm.set_prop_raw(&o, b"name", Value::Str(func.name.clone()));
            let cn = func.scope().map(|c| c.name.clone()).unwrap_or_default();
            vm.set_prop_raw(&o, b"class", Value::Str(cn));
        }
        Refl::Prop { cls, name, dynamic } => {
            vm.set_prop_raw(&o, b"name", Value::Str(name.clone()));
            let decl = if *dynamic {
                cls.name.clone()
            } else {
                prop_decl_class(cls, name)
                    .map(|c| unsafe { (*c).name.clone() })
                    .unwrap_or(cls.name.clone())
            };
            vm.set_prop_raw(&o, b"class", Value::Str(decl));
        }
        Refl::Const { cls, name } => {
            vm.set_prop_raw(&o, b"name", Value::Str(name.clone()));
            let decl = cls
                .find_const(name)
                .map(|c| unsafe { (*c.class).name.clone() })
                .unwrap_or(cls.name.clone());
            vm.set_prop_raw(&o, b"class", Value::Str(decl));
        }
        Refl::Param { func, idx, .. } => {
            let ps = vm.params_of(func).to_vec();
            if let Some(p) = ps.get(*idx) {
                vm.set_prop_raw(&o, b"name", Value::Str(p.name.clone()));
            }
        }
        Refl::Extension(n) | Refl::GlobalConst(n) => {
            vm.set_prop_raw(&o, b"name", Value::Str(n.clone()))
        }
        _ => {}
    }
    set_data(&o, r);
    Ok(o)
}

fn func_name(f: &Func, closure: bool) -> PhpStr {
    match &f.kind {
        FuncKind::User(c) if closure && c.flags & fflags::CLOSURE != 0 => c.name.clone(),
        _ => f.name.clone(),
    }
}

fn prop_decl_class(cls: &Class, name: &PhpStr) -> Option<*const Class> {
    let map = cls.prop_map.borrow();
    if let Some(idxs) = map.get(name) {
        return Some(cls.props.borrow()[idxs[0] as usize].class);
    }
    cls.find_static(name).map(|s| s.class)
}

/// Class from an object or name argument.
fn class_arg(vm: &mut Vm, v: &Value) -> R<Rc<Class>> {
    match v {
        Value::Object(o) => Ok(vm.class_of(o)),
        Value::Str(s) => match vm.find_class(s, true)? {
            Some(c) => Ok(c),
            None => refl_error(
                vm,
                &format!("Class \"{}\" does not exist", s.to_string_lossy()),
            ),
        },
        other => {
            let s = vm.to_php_str(other)?;
            match vm.find_class(&s, true)? {
                Some(c) => Ok(c),
                None => refl_error(
                    vm,
                    &format!("Class \"{}\" does not exist", s.to_string_lossy()),
                ),
            }
        }
    }
}

fn this_class(a: &Args) -> Option<Rc<Class>> {
    data(a.this_obj(), |r| match r {
        Refl::Class(c) | Refl::Object(c, _) => Some(c.clone()),
        _ => None,
    })
}

macro_rules! need_class {
    ($vm:expr, $a:expr) => {
        match this_class($a) {
            Some(c) => c,
            None => {
                return refl_error(
                    $vm,
                    "Internal error: Failed to retrieve the reflection object",
                )
            }
        }
    };
}

fn noop(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}
fn ret_false(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(false))
}
fn ret_true(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Bool(true))
}
fn ret_null(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Null)
}
fn empty_array(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::Array(PhpArray::new()))
}

fn modifier_names(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let m = a.int(0);
    let mut out = PhpArray::new();
    let mut add = |s: &str| {
        let _ = out.push(Value::from(s));
    };
    if m & (64 | 16) != 0 && m & 16 == 0 || m & 64 != 0 {
        add("abstract");
    }
    if m & 32 != 0 {
        add("final");
    }
    if m & 512 != 0 {
        add("virtual");
    }
    if m & 1 != 0 {
        add("public");
    } else if m & 2 != 0 {
        add("protected");
    } else if m & 4 != 0 {
        add("private");
    }
    if m & 4096 != 0 {
        add("private(set)");
    } else if m & 2048 != 0 {
        add("protected(set)");
    }
    if m & 16 != 0 {
        add("static");
    }
    if m & (128 | 65536) != 0 {
        add("readonly");
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- functions

fn this_func(a: &Args) -> Option<(Rc<Func>, Option<ObjRef>, Option<Rc<Class>>)> {
    data(a.this_obj(), |r| match r {
        Refl::Func { func, closure } => Some((func.clone(), closure.clone(), None)),
        Refl::Method { func, cls } => Some((func.clone(), None, Some(cls.clone()))),
        _ => None,
    })
}

macro_rules! need_func {
    ($vm:expr, $a:expr) => {
        match this_func($a) {
            Some(f) => f,
            None => {
                return refl_error(
                    $vm,
                    "Internal error: Failed to retrieve the reflection object",
                )
            }
        }
    };
}

fn f_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let v = a.val(0);
    let o = a.this_obj().clone();
    match &v {
        Value::Object(c) => {
            let Some(cb) = vm.closure_data(c) else {
                return vm.type_error("ReflectionFunction::__construct(): Argument #1 ($function) must be of type Closure|string, object given");
            };
            let r = Refl::Func {
                func: cb.func.clone(),
                closure: Some(c.clone()),
            };
            let n = func_name(&cb.func, true);
            vm.set_prop_raw(&o, b"name", Value::Str(n));
            set_data(&o, r);
        }
        _ => {
            let n = vm.to_php_str(&v)?;
            let name = n.strip_prefix(b"\\").unwrap_or(&n);
            match vm.functions.get(&lower(name)).cloned() {
                Some(f) => {
                    vm.set_prop_raw(&o, b"name", Value::Str(f.name.clone()));
                    set_data(
                        &o,
                        Refl::Func {
                            func: f,
                            closure: None,
                        },
                    );
                }
                None => {
                    return refl_error(
                        vm,
                        &format!(
                            "Function {}() does not exist",
                            String::from_utf8_lossy(name)
                        ),
                    );
                }
            }
        }
    }
    Ok(Value::Null)
}

fn code_of(f: &Func) -> Option<&Rc<FuncCode>> {
    f.code()
}

fn fa_in_namespace(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(f.name.contains(&b'\\')))
}
fn fa_is_closure(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    Ok(Value::Bool(
        c.is_some() && code_of(&f).is_some_and(|c| c.flags & fflags::CLOSURE != 0),
    ))
}
fn fa_is_deprecated(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(
        code_of(&f).is_some_and(|c| c.flags & fflags::DEPRECATED != 0),
    ))
}
fn fa_is_internal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(
        f.is_native() || f.scope().is_some_and(|c| c.has_flag(cflags::INTERNAL)),
    ))
}
fn fa_is_user(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(
        !(f.is_native() || f.scope().is_some_and(|c| c.has_flag(cflags::INTERNAL))),
    ))
}
fn fa_is_generator(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(code_of(&f).is_some_and(|c| c.is_generator())))
}
fn fa_is_variadic(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    let ps = vm.params_of(&f).to_vec();
    Ok(Value::Bool(ps.last().is_some_and(|p| p.variadic)))
}
fn fa_is_static(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let st = f.flags & mflags::STATIC != 0
        || code_of(&f).is_some_and(|c| c.flags & fflags::STATIC_CLOSURE != 0);
    let _ = c;
    Ok(Value::Bool(st))
}

fn closure_cd<T>(c: &ObjRef, f: impl FnOnce(&ClosureData) -> T) -> Option<T> {
    let n = c.obj().native.borrow();
    let any: &dyn std::any::Any = &**n.as_ref()?;
    any.downcast_ref::<ClosureData>().map(f)
}

fn fa_closure_this(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c, _) = need_func!(vm, a);
    Ok(c.and_then(|c| closure_cd(&c, |d| d.this.clone()).flatten())
        .map(Value::Object)
        .unwrap_or_default())
}
fn fa_closure_scope(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c, _) = need_func!(vm, a);
    let p = c.and_then(|c| closure_cd(&c, |d| d.scope));
    match p {
        Some(p) if !p.is_null() => {
            let cls = vm.rc_class(p);
            Ok(Value::Object(new_refl(
                vm,
                "ReflectionClass",
                Refl::Class(cls),
            )?))
        }
        _ => Ok(Value::Null),
    }
}
fn fa_closure_called(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, c, _) = need_func!(vm, a);
    let p = c.and_then(|c| closure_cd(&c, |d| d.called_scope));
    match p {
        Some(p) if !p.is_null() => {
            let cls = vm.rc_class(p);
            Ok(Value::Object(new_refl(
                vm,
                "ReflectionClass",
                Refl::Class(cls),
            )?))
        }
        _ => Ok(Value::Null),
    }
}
fn fa_closure_used(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let mut out = PhpArray::new();
    if let (Some(c), Some(code)) = (c, code_of(&f)) {
        closure_cd(&c, |d| {
            for (k, (inner, _, _)) in code.uses.iter().enumerate() {
                if let Some(Some(v)) = d.captured.get(k) {
                    out.insert(Key::Str(code.cv_names[*inner as usize].clone()), v.clone());
                }
            }
        });
    }
    Ok(Value::Array(out))
}
fn fa_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(code_of(&f)
        .and_then(|c| c.doc_comment.clone())
        .map(Value::Str)
        .unwrap_or(Value::Bool(false)))
}
fn fa_end_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(user_only(&f, |c| Value::Int(c.line_end as i64)))
}
fn fa_start_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(user_only(&f, |c| Value::Int(c.line_start as i64)))
}
fn fa_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(user_only(&f, |c| Value::Str(c.file.clone())))
}

fn user_only(f: &Func, g: impl FnOnce(&FuncCode) -> Value) -> Value {
    if f.scope().is_some_and(|c| c.has_flag(cflags::INTERNAL)) {
        return Value::Bool(false);
    }
    match code_of(f) {
        Some(c) => g(c),
        None => Value::Bool(false),
    }
}

fn fa_extension(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    if !f.is_native() {
        return Ok(Value::Null);
    }
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionExtension",
        Refl::Extension(PhpStr::from("standard")),
    )?))
}
fn fa_extension_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    if !f.is_native() {
        return Ok(Value::Bool(false));
    }
    let name = vm
        .modules
        .iter()
        .find(|m| {
            m.functions.iter().any(|(s, _)| {
                crate::registry::sig_name(s).eq_ignore_ascii_case(&f.name.to_string_lossy())
            })
        })
        .map(|m| m.name)
        .unwrap_or("Core");
    Ok(Value::from(name))
}
fn fa_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    Ok(Value::Str(func_name(&f, c.is_some())))
}
fn split_ns(n: &[u8]) -> (&[u8], &[u8]) {
    match n.iter().rposition(|&c| c == b'\\') {
        Some(p) => (&n[..p], &n[p + 1..]),
        None => (b"", n),
    }
}
fn fa_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let n = func_name(&f, c.is_some());
    if n.starts_with(b"{closure") {
        let ns = f.code().and_then(|code| code.class_name.clone());
        let _ = ns;
        return Ok(Value::Str(PhpStr::empty()));
    }
    Ok(Value::Str(PhpStr::new(split_ns(&n).0)))
}
fn fa_short(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let n = func_name(&f, c.is_some());
    if n.starts_with(b"{closure") {
        return Ok(Value::Str(n));
    }
    Ok(Value::Str(PhpStr::new(split_ns(&n).1)))
}
fn fa_nparams(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Int(vm.params_of(&f).len() as i64))
}
fn fa_nreq(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    let n = match &f.kind {
        FuncKind::User(c) => c.num_required,
        FuncKind::Native { .. } => vm.native_sig(&f).num_required,
    };
    Ok(Value::Int(n as i64))
}
fn fa_params(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let n = vm.params_of(&f).len();
    let mut out = PhpArray::new();
    for i in 0..n {
        let o = new_refl(
            vm,
            "ReflectionParameter",
            Refl::Param {
                func: f.clone(),
                closure: c.clone(),
                idx: i,
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}
fn fa_statics(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let mut out = PhpArray::new();
    let Some(code) = code_of(&f) else {
        return Ok(Value::Array(out));
    };
    // Names of static variables from the BindStatic instructions.
    let mut names: Vec<(u32, PhpStr)> = Vec::new();
    for op in &code.ops {
        if let crate::bytecode::Op::BindStatic { var, index, .. } = op {
            names.push((
                *index,
                code.cv_names
                    .get(*var as usize)
                    .cloned()
                    .unwrap_or_default(),
            ));
        }
    }
    let cells: Vec<Option<goro_runtime::PhpRef>> = match &c {
        Some(cl) => closure_cd(cl, |d| d.statics.borrow().clone()).unwrap_or_default(),
        None => f.statics.borrow().clone(),
    };
    for (idx, name) in names {
        let v = cells.get(idx as usize).cloned().flatten().map(|r| r.get());
        let v = match v {
            Some(v) => v,
            None => {
                // Not yet initialized: the initializer if constant.
                Value::Null
            }
        };
        out.insert(Key::Str(name), v);
    }
    // Closure use variables come too.
    if let Some(cl) = c {
        closure_cd(&cl, |d| {
            for (k, (inner, _, _)) in code.uses.iter().enumerate() {
                if let Some(Some(v)) = d.captured.get(k) {
                    out.insert(Key::Str(code.cv_names[*inner as usize].clone()), v.deref());
                }
            }
        });
    }
    Ok(Value::Array(out))
}
fn fa_by_ref(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(match &f.kind {
        FuncKind::User(c) => c.flags & fflags::BY_REF != 0,
        FuncKind::Native { .. } => vm.native_sig(&f).by_ref_return,
    }))
}
fn ret_type_of(vm: &mut Vm, f: &Func) -> Option<TypeHint> {
    match &f.kind {
        FuncKind::User(c) => c.ret_type.clone(),
        FuncKind::Native { .. } => vm.native_sig(f).ret.clone(),
    }
}
fn fa_has_ret(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(ret_type_of(vm, &f).is_some()))
}
fn fa_ret(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    match ret_type_of(vm, &f) {
        Some(t) => Ok(Value::Object(new_type(vm, t, f.scope.get())?)),
        None => Ok(Value::Null),
    }
}

fn fa_attrs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    let attrs: Vec<AttrInfo> = code_of(&f).map(|c| c.attrs.to_vec()).unwrap_or_default();
    let target = if f.scope().is_some() { 4 } else { 2 };
    attributes_array(vm, a, &attrs, target, f.scope.get(), 0)
}

fn f_is_anon(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    Ok(Value::Bool(
        c.is_some() && func_name(&f, true).starts_with(b"{closure"),
    ))
}

fn f_invoke(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let args = a.vals.clone();
    let callee = match &c {
        Some(cl) => vm.closure_data(cl).unwrap(),
        None => Callee::func(f),
    };
    vm.call_callee(callee, args)
}

fn f_invoke_args(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    let args = a.array(0);
    let callee = match &c {
        Some(cl) => vm.closure_data(cl).unwrap(),
        None => Callee::func(f),
    };
    vm.call_callee_named(callee, args)
}

fn f_get_closure(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _) = need_func!(vm, a);
    if let Some(cl) = c {
        return Ok(Value::Object(cl));
    }
    let o = vm.closure_from_callee(Callee::func(f))?;
    Ok(Value::Object(o))
}

fn param_string(vm: &mut Vm, p: &ParamInfo, i: usize, required: bool) -> String {
    let mut s = format!(
        "Parameter #{} [ <{}> ",
        i,
        if required { "required" } else { "optional" }
    );
    if let Some(t) = &p.ty {
        s.push_str(&t.display());
        s.push(' ');
    }
    if p.by_ref {
        s.push('&');
    }
    if p.variadic {
        s.push_str("...");
    }
    s.push('$');
    s.push_str(&p.name.to_string_lossy());
    let names_const = p.default_text.as_deref().is_some_and(text_has_name);
    match (&p.default, &p.default_text) {
        (ParamDefault::Const(v), t) if !names_const || t.is_none() => {
            s.push_str(" = ");
            s.push_str(&render_value(vm, v));
        }
        (_, Some(d)) => {
            s.push_str(" = ");
            s.push_str(&render_expr_text(d));
        }
        _ => {}
    }
    s.push_str(" ]");
    s
}

/// Whether default-value source text names a constant or class (outside
/// string literals): Reflection then shows the text, else the value.
fn text_has_name(t: &str) -> bool {
    let b = t.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'\'' || c == b'"' {
            i += 1;
            while i < b.len() && b[i] != c {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c == b'\\' || c >= 0x80 {
            let st = i;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'\\' || b[i] >= 0x80)
            {
                i += 1;
            }
            let w = t[st..i].to_ascii_lowercase();
            if !matches!(w.as_str(), "null" | "true" | "false" | "array") {
                return true;
            }
            continue;
        }
        if c.is_ascii_digit() {
            // Numbers, hex digits and exponents included.
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    false
}

/// A default value as Reflection prints it.
fn render_value(vm: &mut Vm, v: &Value) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Array(a) => {
            let list = a
                .iter()
                .enumerate()
                .all(|(i, (k, _))| matches!(k, goro_runtime::KeyRef::Int(n) if n == i as i64));
            let mut parts = Vec::new();
            for (k, x) in a.iter() {
                let xv = render_value(vm, &x.deref());
                if list {
                    parts.push(xv);
                } else {
                    let ks = match k {
                        goro_runtime::KeyRef::Int(n) => n.to_string(),
                        goro_runtime::KeyRef::Str(s) => render_value(vm, &Value::Str(s.clone())),
                    };
                    parts.push(format!("{} => {}", ks, xv));
                }
            }
            format!("[{}]", parts.join(", "))
        }
        _ => {
            let mut out = Vec::new();
            let _ = vm.var_export_into(&mut out, v, 0);
            String::from_utf8_lossy(&out).into_owned()
        }
    }
}

/// Non-constant default source text, normalized the way Reflection shows
/// expressions (`new \Foo()`).
fn render_expr_text(t: &str) -> String {
    let tt = t.trim();
    if let Some(rest) = tt.strip_prefix("new ") {
        let rest = rest.trim();
        let (name, args) = match rest.find('(') {
            Some(p) => (rest[..p].trim(), &rest[p..]),
            None => (rest, "()"),
        };
        let name = if name.starts_with('\\') {
            name.to_string()
        } else {
            format!("\\{}", name)
        };
        return format!("new {}{}", name, args);
    }
    tt.to_string()
}

fn f_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, m) = need_func!(vm, a);
    let params = vm.params_of(&f).to_vec();
    let nreq = match &f.kind {
        FuncKind::User(code) => code.num_required as usize,
        FuncKind::Native { .. } => vm.native_sig(&f).num_required as usize,
    };
    let mut s = String::new();
    let kind = if m.is_some() {
        "Method"
    } else if c.is_some() {
        "Closure"
    } else {
        "Function"
    };
    let internal = f.is_native();
    s.push_str(&format!(
        "{} [ <{}> ",
        kind,
        if internal {
            "internal:standard"
        } else {
            "user"
        }
    ));
    if m.is_some() {
        let mods = method_modifiers(&f);
        let names: Vec<&str> = [
            (mods & 64 != 0, "abstract"),
            (mods & 32 != 0, "final"),
            (mods & 16 != 0, "static"),
        ]
        .iter()
        .filter(|x| x.0)
        .map(|x| x.1)
        .collect();
        for n in names {
            s.push_str(n);
            s.push(' ');
        }
        s.push_str(if mods & 4 != 0 {
            "private"
        } else if mods & 2 != 0 {
            "protected"
        } else {
            "public"
        });
        s.push_str(" method ");
    } else {
        s.push_str("function ");
    }
    s.push_str(&func_name(&f, c.is_some()).to_string_lossy());
    s.push_str(" ] {\n");
    if let Some(code) = code_of(&f)
        && !internal
    {
        s.push_str(&format!(
            "  @@ {} {} - {}\n",
            code.file.to_string_lossy(),
            code.line_start,
            code.line_end
        ));
    }
    if !params.is_empty() {
        s.push_str(&format!("\n  - Parameters [{}] {{\n", params.len()));
        for (i, p) in params.iter().enumerate() {
            let ps = param_string(vm, p, i, i < nreq);
            s.push_str(&format!("    {}\n", ps));
        }
        s.push_str("  }\n");
    }
    if let Some(t) = ret_type_of(vm, &f) {
        s.push_str(&format!("  - Return [ {} ]\n", t.display()));
    }
    s.push_str("}\n");
    Ok(Value::Str(PhpStr::from(s)))
}

// ---------------------------------------------------------------- methods

fn find_method_in(vm: &mut Vm, cls: &Rc<Class>, name: &[u8]) -> R<Rc<Func>> {
    let ln = lower(name);
    if let Some(f) = cls.find_method_str(&ln) {
        return Ok(f);
    }
    if cls.lname.as_bytes() == b"closure" && ln.as_bytes() == b"__invoke" {
        if let Some(f) = cls.find_method_str(&ln) {
            return Ok(f);
        }
    }
    refl_error(
        vm,
        &format!(
            "Method {}::{}() does not exist",
            cls.name.dname(),
            String::from_utf8_lossy(name)
        ),
    )
}

fn m_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let first = a.val(0);
    let (cls, mname) = if a.has(1) && !a.val(1).is_null() {
        (class_arg(vm, &first)?, a.str(1))
    } else {
        let s = vm.to_php_str(&first)?;
        match s.windows(2).position(|w| w == b"::") {
            Some(p) => {
                let c = PhpStr::new(&s[..p]);
                let m = PhpStr::new(&s[p + 2..]);
                (class_arg(vm, &Value::Str(c))?, m)
            }
            None => {
                return refl_error(
                    vm,
                    "ReflectionMethod::__construct(): Argument #1 ($objectOrMethod) must be a valid method name",
                );
            }
        }
    };
    if a.has(1) && a.val(1).is_null() {
        vm.deprecated("Calling ReflectionMethod::__construct() with 1 argument is deprecated, use ReflectionMethod::createFromMethodName() instead")?;
    }
    let f = find_method_in(vm, &cls, &mname)?;
    vm.set_prop_raw(&o, b"name", Value::Str(f.name.clone()));
    let cn = f.scope().map(|c| c.name.clone()).unwrap_or_default();
    vm.set_prop_raw(&o, b"class", Value::Str(cn));
    set_data(&o, Refl::Method { func: f, cls });
    Ok(Value::Null)
}

fn m_create_from(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let s = a.str(0);
    let Some(p) = s.windows(2).position(|w| w == b"::") else {
        return refl_error(
            vm,
            "ReflectionMethod::createFromMethodName(): Argument #1 ($method) must be a valid method name",
        );
    };
    let cls = class_arg(vm, &Value::Str(PhpStr::new(&s[..p])))?;
    let f = find_method_in(vm, &cls, &s[p + 2..])?;
    let target = if a.static_class.is_null() {
        "ReflectionMethod".to_string()
    } else {
        unsafe { (*a.static_class).name.to_string_lossy() }
    };
    let o = new_refl(vm, &target, Refl::Method { func: f, cls })?;
    Ok(Value::Object(o))
}

fn method_modifiers(f: &Func) -> i64 {
    let mut m = 0;
    if f.flags & mflags::PUBLIC != 0 {
        m |= 1;
    }
    if f.flags & mflags::PROTECTED != 0 {
        m |= 2;
    }
    if f.flags & mflags::PRIVATE != 0 {
        m |= 4;
    }
    if f.flags & mflags::STATIC != 0 {
        m |= 16;
    }
    if f.flags & mflags::FINAL != 0 {
        m |= 32;
    }
    if f.flags & mflags::ABSTRACT != 0 {
        m |= 64;
    }
    m
}

fn m_flag(vm: &mut Vm, a: &Args, flag: u32) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(f.flags & flag != 0))
}
fn m_is_public(vm: &mut Vm, a: &mut Args) -> R<Value> {
    m_flag(vm, a, mflags::PUBLIC)
}
fn m_is_private(vm: &mut Vm, a: &mut Args) -> R<Value> {
    m_flag(vm, a, mflags::PRIVATE)
}
fn m_is_protected(vm: &mut Vm, a: &mut Args) -> R<Value> {
    m_flag(vm, a, mflags::PROTECTED)
}
fn m_is_abstract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    m_flag(vm, a, mflags::ABSTRACT)
}
fn m_is_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    m_flag(vm, a, mflags::FINAL)
}
fn m_is_ctor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(f.lname.as_bytes() == b"__construct"))
}
fn m_is_dtor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Bool(f.lname.as_bytes() == b"__destruct"))
}
fn m_modifiers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _) = need_func!(vm, a);
    Ok(Value::Int(method_modifiers(&f)))
}

fn m_callee(vm: &mut Vm, f: &Rc<Func>, cls: &Rc<Class>, obj: Value, fname: &str) -> R<Callee> {
    if f.flags & mflags::STATIC != 0 {
        return Ok(Callee {
            func: f.clone(),
            this: None,
            static_class: Rc::as_ptr(cls),
            scope: f.scope.get(),
            closure: None,
            magic_name: None,
        });
    }
    let Value::Object(o) = obj else {
        return vm.type_error(&format!(
            "ReflectionMethod::{}(): Argument #1 ($object) must be of type object, {} given",
            fname,
            obj.tname()
        ));
    };
    let oc = vm.class_of(&o);
    let decl = f.scope().unwrap();
    if !oc.is_subclass_of(decl) {
        return refl_error(
            vm,
            "Given object is not an instance of the class this method was declared in",
        );
    }
    if f.flags & mflags::ABSTRACT != 0 {
        return refl_error(
            vm,
            &format!(
                "Trying to invoke abstract method {}::{}()",
                decl.name.dname(),
                f.name.dname()
            ),
        );
    }
    Ok(Callee {
        func: f.clone(),
        this: Some(o),
        static_class: Rc::as_ptr(&oc),
        scope: f.scope.get(),
        closure: None,
        magic_name: None,
    })
}

fn this_method(a: &Args) -> Option<(Rc<Func>, Rc<Class>)> {
    data(a.this_obj(), |r| match r {
        Refl::Method { func, cls } => Some((func.clone(), cls.clone())),
        _ => None,
    })
}

fn m_invoke(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, cls)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let c = m_callee(vm, &f, &cls, a.val(0), "invoke")?;
    let args = a.vals[1.min(a.vals.len())..].to_vec();
    vm.call_callee(c, args)
}

fn m_invoke_args(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, cls)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let c = m_callee(vm, &f, &cls, a.val(0), "invokeArgs")?;
    let args = a.array(1);
    vm.call_callee_named(c, args)
}

fn m_get_closure(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, cls)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let c = if f.flags & mflags::STATIC != 0 {
        m_callee(vm, &f, &cls, Value::Null, "getClosure")?
    } else {
        let v = a.val(0);
        if v.is_null() {
            return refl_error(
                vm,
                "ReflectionMethod::getClosure(): Argument #1 ($object) must be provided for non-static methods",
            );
        }
        m_callee(vm, &f, &cls, v, "getClosure")?
    };
    let o = vm.closure_from_callee(c)?;
    Ok(Value::Object(o))
}

fn m_declaring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, _)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let c = vm.rc_class(f.scope.get());
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionClass",
        Refl::Class(c),
    )?))
}

fn m_prototype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, _)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let proto = f.proto.borrow().clone();
    match proto {
        Some(p) => {
            let mut root = p;
            loop {
                let next = root.proto.borrow().clone();
                match next {
                    Some(n) => root = n,
                    None => break,
                }
            }
            let c = vm.rc_class(root.scope.get());
            Ok(Value::Object(new_refl(
                vm,
                "ReflectionMethod",
                Refl::Method { func: root, cls: c },
            )?))
        }
        None => refl_error(
            vm,
            &format!(
                "Method {}::{} does not have a prototype",
                f.scope().map(|c| c.name.dname()).unwrap_or_default(),
                f.name.dname()
            ),
        ),
    }
}

fn m_has_prototype(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((f, _)) = this_method(a) else {
        return Ok(Value::Null);
    };
    let _ = vm;
    Ok(Value::Bool(f.proto.borrow().is_some()))
}

// ---------------------------------------------------------------- classes

fn c_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = a.val(0);
    let cls = class_arg(vm, &v)?;
    vm.set_prop_raw(&o, b"name", Value::Str(cls.name.clone()));
    let mut is_obj = false;
    let mut k = Some(vm.class_of(&o));
    while let Some(c) = k {
        if c.lname.as_bytes() == b"reflectionobject" {
            is_obj = true;
            break;
        }
        k = c.parent.clone();
    }
    match v {
        Value::Object(ob) if is_obj => set_data(&o, Refl::Object(cls, ob)),
        _ => set_data(&o, Refl::Class(cls)),
    }
    Ok(Value::Null)
}

fn this_object(a: &Args) -> Option<ObjRef> {
    data(a.this_obj(), |r| match r {
        Refl::Object(_, o) => Some(o.clone()),
        _ => None,
    })
}

fn c_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let cls = need_class!(vm, a);
    let kind = if cls.is_interface() {
        "Interface"
    } else if cls.is_trait() {
        "Trait"
    } else if cls.is_enum() {
        "Enum"
    } else {
        "Class"
    };
    let mut s = format!(
        "{} [ <{}> {}class {}",
        kind,
        if cls.has_flag(cflags::INTERNAL) {
            "internal:Core"
        } else {
            "user"
        },
        if cls.has_flag(cflags::ABSTRACT) {
            "abstract "
        } else if cls.has_flag(cflags::FINAL) {
            "final "
        } else {
            ""
        },
        cls.name.dname()
    );
    if let Some(p) = &cls.parent {
        s.push_str(&format!(" extends {}", p.name.dname()));
    }
    s.push_str(" ] {\n");
    if let Some(f) = &cls.file
        && !cls.has_flag(cflags::INTERNAL)
    {
        s.push_str(&format!(
            "  @@ {} {}-{}\n",
            f.to_string_lossy(),
            cls.line_start,
            cls.line_end
        ));
    }
    s.push_str("}\n");
    Ok(Value::Str(PhpStr::from(s)))
}

fn c_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Str(c.name.clone()))
}
fn c_is_internal(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.has_flag(cflags::INTERNAL)))
}
fn c_is_user(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(!c.has_flag(cflags::INTERNAL)))
}
fn c_is_anon(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.has_flag(cflags::ANON)))
}
fn c_is_instantiable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.kind != ClassKind::Class || c.has_flag(cflags::ABSTRACT) {
        return Ok(Value::Bool(false));
    }
    let ctor = c.magic.borrow().construct.clone();
    Ok(Value::Bool(
        ctor.is_none_or(|f| f.flags & mflags::PUBLIC != 0),
    ))
}
fn c_is_cloneable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.kind != ClassKind::Class
        || c.has_flag(cflags::ABSTRACT)
        || c.has_flag(cflags::NOT_CLONEABLE)
    {
        return Ok(Value::Bool(false));
    }
    let cl = c.magic.borrow().clone.clone();
    Ok(Value::Bool(
        cl.is_none_or(|f| f.flags & mflags::PUBLIC != 0),
    ))
}
fn c_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.has_flag(cflags::INTERNAL) {
        return Ok(Value::Bool(false));
    }
    Ok(c.file.clone().map(Value::Str).unwrap_or(Value::Bool(false)))
}
fn c_start(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.has_flag(cflags::INTERNAL) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(c.line_start as i64))
}
fn c_end(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.has_flag(cflags::INTERNAL) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Int(c.line_end as i64))
}
fn c_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(c.doc.clone().map(Value::Str).unwrap_or(Value::Bool(false)))
}
fn c_ctor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let ctor = c.magic.borrow().construct.clone();
    match ctor {
        Some(f) => Ok(Value::Object(new_refl(
            vm,
            "ReflectionMethod",
            Refl::Method { func: f, cls: c },
        )?)),
        None => Ok(Value::Null),
    }
}
fn c_has_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = lower(&a.str(0));
    Ok(Value::Bool(c.find_method_str(&n).is_some()))
}
fn c_get_method(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    let f = find_method_in(vm, &c, &n)?;
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionMethod",
        Refl::Method { func: f, cls: c },
    )?))
}
fn c_methods(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let filter = a.opt_int(0);
    let mut out = PhpArray::new();
    let order = c.method_order.borrow().clone();
    for ln in order {
        let f = c.methods.borrow().get(&ln).cloned().unwrap();
        if let Some(fl) = filter
            && method_modifiers(&f) & fl == 0
        {
            continue;
        }
        let o = new_refl(
            vm,
            "ReflectionMethod",
            Refl::Method {
                func: f,
                cls: c.clone(),
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}
fn c_has_prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    if c.prop_map.borrow().contains_key(&n) || c.find_static(&n).is_some() {
        return Ok(Value::Bool(true));
    }
    // ReflectionObject also sees dynamic properties.
    let robj = vm.get_prop_raw(a.this_obj(), b"name");
    let _ = robj;
    Ok(Value::Bool(false))
}
fn c_get_prop(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    if let Some(p) = n.windows(2).position(|w| w == b"::") {
        let c2 = class_arg(vm, &Value::Str(PhpStr::new(&n[..p])))?;
        let pn = PhpStr::new(&n[p + 2..]);
        if !c.is_subclass_of(&c2) {
            return refl_error(
                vm,
                &format!(
                    "Fully qualified property name {}::${} does not specify a base class of {}",
                    c2.name.dname(),
                    pn.dname(),
                    c.name.dname()
                ),
            );
        }
        return Ok(Value::Object(new_refl(
            vm,
            "ReflectionProperty",
            Refl::Prop {
                cls: c2,
                name: pn,
                dynamic: false,
            },
        )?));
    }
    if c.prop_map.borrow().contains_key(&n) || c.find_static(&n).is_some() {
        return Ok(Value::Object(new_refl(
            vm,
            "ReflectionProperty",
            Refl::Prop {
                cls: c,
                name: n,
                dynamic: false,
            },
        )?));
    }
    refl_error(
        vm,
        &format!("Property {}::${} does not exist", c.name.dname(), n.dname()),
    )
}

fn prop_modifiers(flags: u32) -> i64 {
    let mut m = 0;
    if flags & mflags::PUBLIC != 0 {
        m |= 1;
    }
    if flags & mflags::PROTECTED != 0 {
        m |= 2;
    }
    if flags & mflags::PRIVATE != 0 {
        m |= 4;
    }
    if flags & mflags::STATIC != 0 {
        m |= 16;
    }
    if flags & mflags::FINAL != 0 {
        m |= 32;
    }
    if flags & mflags::ABSTRACT != 0 {
        m |= 64;
    }
    if flags & mflags::READONLY != 0 {
        m |= 128;
    }
    if flags & mflags::VIRTUAL != 0 {
        m |= 512;
    }
    if flags & mflags::PROTECTED_SET != 0 {
        m |= 2048;
    }
    if flags & mflags::PRIVATE_SET != 0 {
        m |= 4096;
    }
    // readonly implies protected(set) unless declared otherwise.
    if flags & mflags::READONLY != 0
        && flags & (mflags::PRIVATE_SET | mflags::PROTECTED_SET | mflags::PUBLIC_SET) == 0
        && flags & mflags::PUBLIC != 0
    {
        m |= 2048;
    }
    m
}

fn c_props(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let filter = a.opt_int(0);
    let mut out = PhpArray::new();
    let mut seen: Vec<PhpStr> = Vec::new();
    // Own properties first, then inherited (PHP's order).
    let props: Vec<_> = c
        .props
        .borrow()
        .iter()
        .filter(|p| p.flags & mflags::HIDDEN == 0)
        .cloned()
        .collect();
    let cp = Rc::as_ptr(&c);
    let mut ordered: Vec<_> = props
        .iter()
        .filter(|p| std::ptr::eq(p.class, cp))
        .cloned()
        .collect();
    ordered.extend(
        props
            .iter()
            .filter(|p| !std::ptr::eq(p.class, cp) && p.flags & mflags::PRIVATE == 0)
            .cloned(),
    );
    for p in ordered {
        if seen.contains(&p.name) {
            continue;
        }
        if let Some(fl) = filter
            && prop_modifiers(p.flags) & fl == 0
        {
            continue;
        }
        seen.push(p.name.clone());
        let o = new_refl(
            vm,
            "ReflectionProperty",
            Refl::Prop {
                cls: c.clone(),
                name: p.name.clone(),
                dynamic: false,
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    let statics = c.static_props.borrow().clone();
    for sp in statics {
        if seen.contains(&sp.name)
            || (sp.flags & mflags::PRIVATE != 0 && !std::ptr::eq(sp.class, cp))
        {
            continue;
        }
        if let Some(fl) = filter
            && prop_modifiers(sp.flags) & fl == 0
        {
            continue;
        }
        seen.push(sp.name.clone());
        let o = new_refl(
            vm,
            "ReflectionProperty",
            Refl::Prop {
                cls: c.clone(),
                name: sp.name.clone(),
                dynamic: false,
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    // ReflectionObject: dynamic properties of the object.
    if let Some(obj) = this_object(a)
        && filter.is_none_or(|f| f & 1 != 0)
    {
        for (k, _) in obj.obj().props.borrow().iter() {
            let name = match k {
                goro_runtime::KeyRef::Str(s) => s.clone(),
                goro_runtime::KeyRef::Int(i) => PhpStr::from(i.to_string()),
            };
            let o = new_refl(
                vm,
                "ReflectionProperty",
                Refl::Prop {
                    cls: c.clone(),
                    name,
                    dynamic: true,
                },
            )?;
            let _ = out.push(Value::Object(o));
        }
    }
    Ok(Value::Array(out))
}
fn c_has_const(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    Ok(Value::Bool(c.find_const(&n).is_some()))
}
fn const_modifiers(flags: u32) -> i64 {
    let mut m = 0;
    if flags & mflags::PUBLIC != 0 {
        m |= 1;
    }
    if flags & mflags::PROTECTED != 0 {
        m |= 2;
    }
    if flags & mflags::PRIVATE != 0 {
        m |= 4;
    }
    if flags & mflags::FINAL != 0 {
        m |= 32;
    }
    m
}
fn c_consts(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let filter = a.opt_int(0);
    let consts = c.consts.borrow().clone();
    let mut out = PhpArray::new();
    for k in consts {
        if let Some(f) = filter
            && const_modifiers(k.flags) & f == 0
        {
            continue;
        }
        let v = if k.flags & mflags::CASE != 0 {
            vm.enum_case(&c, &k.name)?
        } else {
            vm.eval_class_const(&k)?
        };
        out.insert(Key::Str(k.name.clone()), v);
    }
    Ok(Value::Array(out))
}
fn c_refl_consts(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let filter = a.opt_int(0);
    let consts = c.consts.borrow().clone();
    let mut out = PhpArray::new();
    for k in consts {
        if let Some(f) = filter
            && const_modifiers(k.flags) & f == 0
        {
            continue;
        }
        let o = new_refl(
            vm,
            "ReflectionClassConstant",
            Refl::Const {
                cls: c.clone(),
                name: k.name.clone(),
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}
fn c_const(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    match c.find_const(&n) {
        Some(k) => {
            if k.flags & mflags::CASE != 0 {
                vm.enum_case(&c, &n)
            } else {
                vm.eval_class_const(&k)
            }
        }
        None => Ok(Value::Bool(false)),
    }
}
fn c_refl_const(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    if c.find_const(&n).is_none() {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionClassConstant",
        Refl::Const { cls: c, name: n },
    )?))
}
fn c_interfaces(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let mut out = PhpArray::new();
    let ifs = c.interfaces.borrow().clone();
    for i in ifs {
        let o = new_refl(vm, "ReflectionClass", Refl::Class(i.clone()))?;
        out.insert(Key::Str(i.name.clone()), Value::Object(o));
    }
    Ok(Value::Array(out))
}
fn c_interface_names(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Array(PhpArray::from_values(
        c.interfaces
            .borrow()
            .iter()
            .map(|i| Value::Str(i.name.clone())),
    )))
}
fn c_is_interface(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.is_interface()))
}
fn c_traits(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let mut out = PhpArray::new();
    let ts = c.traits.borrow().clone();
    for t in ts {
        let o = new_refl(vm, "ReflectionClass", Refl::Class(t.clone()))?;
        out.insert(Key::Str(t.name.clone()), Value::Object(o));
    }
    Ok(Value::Array(out))
}
fn c_trait_names(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Array(PhpArray::from_values(
        c.traits.borrow().iter().map(|i| Value::Str(i.name.clone())),
    )))
}
fn c_trait_aliases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let mut out = PhpArray::new();
    if let Some(d) = &c.decl {
        for tu in &d.traits {
            for r in &tu.rules {
                if let crate::class::TraitRule::Alias {
                    trait_,
                    method,
                    alias: Some(al),
                    ..
                } = r
                {
                    let t = trait_.clone().or_else(|| {
                        tu.traits
                            .iter()
                            .find(|tn| {
                                vm.classes
                                    .get(&lower(tn))
                                    .is_some_and(|tc| tc.find_method_str(&lower(method)).is_some())
                            })
                            .cloned()
                    });
                    let t = t.unwrap_or_default();
                    out.insert(
                        Key::Str(al.clone()),
                        Value::Str(PhpStr::from(format!(
                            "{}::{}",
                            t.to_string_lossy(),
                            method.to_string_lossy()
                        ))),
                    );
                }
            }
        }
    }
    Ok(Value::Array(out))
}
fn c_is_trait(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.is_trait()))
}
fn c_is_enum(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.is_enum()))
}
fn c_is_abstract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let abs = c.has_flag(cflags::ABSTRACT)
        || c.is_interface()
        || c.methods
            .borrow()
            .values()
            .any(|f| f.flags & mflags::ABSTRACT != 0)
            && c.kind != ClassKind::Class;
    Ok(Value::Bool(
        abs || (c.kind == ClassKind::Trait
            && c.methods
                .borrow()
                .values()
                .any(|f| f.flags & mflags::ABSTRACT != 0)),
    ))
}
fn c_is_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.has_flag(cflags::FINAL)))
}
fn c_is_readonly(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.has_flag(cflags::READONLY)))
}
fn c_modifiers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let mut m = 0;
    if c.has_flag(cflags::ABSTRACT) {
        m |= 64;
    }
    if c.has_flag(cflags::FINAL) {
        m |= 32;
    }
    if c.has_flag(cflags::READONLY) {
        m |= 65536;
    }
    Ok(Value::Int(m))
}
fn c_is_instance(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let Some(o) = a.obj(0) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(vm.class_of(&o).is_subclass_of(&c)))
}
fn c_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let args = a.vals.clone();
    let named = a.named.take();
    let mut arr = PhpArray::from_values(args);
    if let Some(n) = named {
        for (k, v) in n.iter() {
            arr.insert(k.to_owned(), v.clone());
        }
    }
    new_with_args(vm, &c, arr)
}
fn new_with_args(vm: &mut Vm, c: &Rc<Class>, args: PhpArray) -> R<Value> {
    let o = vm.instantiate(c)?;
    let ctor = c.magic.borrow().construct.clone();
    match ctor {
        Some(f) => {
            if f.flags & mflags::PUBLIC == 0 {
                return refl_error(
                    vm,
                    &format!(
                        "Access to non-public constructor of class {}",
                        c.name.dname()
                    ),
                );
            }
            let callee = Callee {
                func: f.clone(),
                this: Some(o.clone()),
                static_class: Rc::as_ptr(c),
                scope: f.scope.get(),
                closure: None,
                magic_name: None,
            };
            vm.call_callee_named(callee, args)?;
        }
        None => {
            if !args.is_empty() {
                return refl_error(
                    vm,
                    &format!(
                        "Class {} does not have a constructor, so you cannot pass any constructor arguments",
                        c.name.dname()
                    ),
                );
            }
        }
    }
    Ok(Value::Object(o))
}
fn c_new_no_ctor(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.has_flag(cflags::INTERNAL)
        && c.has_flag(cflags::FINAL)
        && c.lname.as_bytes() != b"stdclass"
    {
        return refl_error(
            vm,
            &format!(
                "Class {} is an internal class marked as final that cannot be instantiated without invoking its constructor",
                c.name.dname()
            ),
        );
    }
    Ok(Value::Object(vm.instantiate(&c)?))
}
fn c_new_args(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let args = a.array(0);
    new_with_args(vm, &c, args)
}
fn c_parent(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    match &c.parent {
        Some(p) => Ok(Value::Object(new_refl(
            vm,
            "ReflectionClass",
            Refl::Class(p.clone()),
        )?)),
        None => Ok(Value::Bool(false)),
    }
}
fn other_class(vm: &mut Vm, v: &Value) -> R<Rc<Class>> {
    if let Value::Object(o) = v
        && let Some(c) = data(o, |r| match r {
            Refl::Class(c) | Refl::Object(c, _) => Some(c.clone()),
            _ => None,
        })
    {
        return Ok(c);
    }
    class_arg(vm, v)
}
fn c_is_subclass(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let o = other_class(vm, &a.val(0))?;
    Ok(Value::Bool(!Rc::ptr_eq(&c, &o) && c.is_subclass_of(&o)))
}
fn c_static_props(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    vm.ensure_statics(&c)?;
    let mut out = PhpArray::new();
    for sp in c.static_props.borrow().iter() {
        let v = sp.cell.get();
        if !vm.is_uninit(&v) {
            out.insert(Key::Str(sp.name.clone()), v);
        }
    }
    Ok(Value::Array(out))
}
fn c_get_static(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    vm.ensure_statics(&c)?;
    match c.find_static(&n) {
        Some(sp) => {
            let v = sp.cell.get();
            if vm.is_uninit(&v) {
                return vm.throw(
                    "Error",
                    &format!(
                        "Typed property {}::${} must not be accessed before initialization",
                        c.name.dname(),
                        n.dname()
                    ),
                );
            }
            Ok(v)
        }
        None => {
            if a.has(1) {
                return Ok(a.val(1));
            }
            refl_error(
                vm,
                &format!("Property {}::${} does not exist", c.name.dname(), n.dname()),
            )
        }
    }
}
fn c_set_static(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    vm.ensure_statics(&c)?;
    match c.find_static(&n) {
        Some(sp) => {
            let mut v = a.val(1);
            if let Some(t) = &sp.ty {
                match vm.check_type(&v, t, false, sp.class, Rc::as_ptr(&c))? {
                    crate::vm::types::Check::Ok => {}
                    crate::vm::types::Check::Coerced(x) => v = x,
                    crate::vm::types::Check::Fail => {
                        return vm.type_error(&format!(
                            "Cannot assign {} to property {}::${} of type {}",
                            v.tname(),
                            c.name.dname(),
                            n.dname(),
                            t.display()
                        ));
                    }
                }
            }
            let old = sp.cell.set(v);
            drop(old);
            Ok(Value::Null)
        }
        None => refl_error(
            vm,
            &format!(
                "Class {} does not have a property named {}",
                c.name.dname(),
                n.dname()
            ),
        ),
    }
}
fn c_default_props(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    vm.ensure_statics(&c)?;
    let mut out = PhpArray::new();
    for sp in c.static_props.borrow().iter() {
        let v = sp.cell.get();
        if !vm.is_uninit(&v) {
            out.insert(Key::Str(sp.name.clone()), v);
        }
    }
    let defaults = vm.class_defaults(&c)?;
    for p in c.props.borrow().iter() {
        let v = &defaults[p.slot as usize];
        if vm.is_uninit(v) || p.flags & mflags::HIDDEN != 0 {
            continue;
        }
        out.insert(Key::Str(p.name.clone()), v.clone());
    }
    Ok(Value::Array(out))
}
fn c_is_iterable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if c.kind != ClassKind::Class || c.has_flag(cflags::ABSTRACT) {
        return Ok(Value::Bool(false));
    }
    Ok(Value::Bool(vm.is_traversable(&c)))
}
fn c_implements(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let i = other_class(vm, &a.val(0))?;
    if !i.is_interface() {
        return refl_error(vm, &format!("{} is not an interface", i.name.dname()));
    }
    Ok(Value::Bool(c.is_subclass_of(&i)))
}
fn c_extension(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if !c.has_flag(cflags::INTERNAL) {
        return Ok(Value::Null);
    }
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionExtension",
        Refl::Extension(PhpStr::from("Core")),
    )?))
}
fn c_extension_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    if !c.has_flag(cflags::INTERNAL) {
        return Ok(Value::Bool(false));
    }
    let name = vm
        .modules
        .iter()
        .find(|m| {
            m.classes
                .iter()
                .any(|(l, _)| l.as_bytes() == c.lname.as_bytes())
        })
        .map(|m| m.name)
        .unwrap_or("Core");
    Ok(Value::from(name))
}
fn c_in_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.name.contains(&b'\\')))
}
fn c_ns(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Str(PhpStr::new(split_ns(&c.name).0)))
}
fn c_short(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Str(PhpStr::new(split_ns(&c.name).1)))
}
fn c_attrs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let attrs = c.attrs.to_vec();
    attributes_array(vm, a, &attrs, 1, Rc::as_ptr(&c), 0)
}

// ---------------------------------------------------------------- properties

fn this_prop(a: &Args) -> Option<(Rc<Class>, PhpStr, bool)> {
    data(a.this_obj(), |r| match r {
        Refl::Prop { cls, name, dynamic } => Some((cls.clone(), name.clone(), *dynamic)),
        _ => None,
    })
}

macro_rules! need_prop {
    ($vm:expr, $a:expr) => {
        match this_prop($a) {
            Some(p) => p,
            None => {
                return refl_error(
                    $vm,
                    "Internal error: Failed to retrieve the reflection object",
                )
            }
        }
    };
}

/// (flags, type, declaring class, default) of a declared or static
/// property.
struct PInfo {
    flags: u32,
    ty: Option<TypeHint>,
    class: *const Class,
    is_static: bool,
    doc: Option<PhpStr>,
    attrs: Vec<AttrInfo>,
    default: Option<ConstExpr>,
    has_hooks: (bool, bool),
    slot: Option<u32>,
}

fn pinfo(cls: &Class, name: &PhpStr) -> Option<PInfo> {
    let map = cls.prop_map.borrow();
    if let Some(idxs) = map.get(name) {
        let p = cls.props.borrow()[idxs[0] as usize].clone();
        return Some(PInfo {
            flags: p.flags,
            ty: p.ty.clone(),
            class: p.class,
            is_static: false,
            doc: p.doc.clone(),
            attrs: p.attrs.to_vec(),
            default: p.default.clone(),
            has_hooks: (p.get.is_some(), p.set.is_some()),
            slot: Some(p.slot),
        });
    }
    cls.find_static(name).map(|s| PInfo {
        flags: s.flags,
        ty: s.ty.clone(),
        class: s.class,
        is_static: true,
        doc: s.doc.clone(),
        attrs: s.attrs.to_vec(),
        default: s
            .init
            .borrow()
            .clone()
            .or_else(|| Some(ConstExpr::Value(s.cell.get()))),
        has_hooks: (false, false),
        slot: None,
    })
}

fn p_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let v = a.val(0);
    let cls = class_arg(vm, &v)?;
    let n = a.str(1);
    let dynamic = if cls.prop_map.borrow().contains_key(&n) || cls.find_static(&n).is_some() {
        false
    } else if let Value::Object(obj) = &v
        && obj.obj().props.borrow().get_str(&n).is_some()
    {
        true
    } else {
        return refl_error(
            vm,
            &format!(
                "Property {}::${} does not exist",
                cls.name.dname(),
                n.dname()
            ),
        );
    };
    vm.set_prop_raw(&o, b"name", Value::Str(n.clone()));
    let decl = if dynamic {
        cls.name.clone()
    } else {
        prop_decl_class(&cls, &n)
            .map(|c| unsafe { (*c).name.clone() })
            .unwrap_or(cls.name.clone())
    };
    vm.set_prop_raw(&o, b"class", Value::Str(decl));
    set_data(
        &o,
        Refl::Prop {
            cls,
            name: n,
            dynamic,
        },
    );
    Ok(Value::Null)
}
fn p_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    let s = match pinfo(&c, &n) {
        Some(p) if !dynamic => {
            let vis = if p.flags & mflags::PRIVATE != 0 {
                "private"
            } else if p.flags & mflags::PROTECTED != 0 {
                "protected"
            } else {
                "public"
            };
            let mut s = format!(
                "Property [ {}{}",
                vis,
                if p.is_static { " static" } else { "" }
            );
            if p.flags & mflags::READONLY != 0 {
                s.push_str(" readonly");
            }
            if let Some(t) = &p.ty {
                s.push(' ');
                s.push_str(&t.display());
            }
            s.push_str(&format!(" ${} ]\n", n.to_string_lossy()));
            s
        }
        _ => format!("Property [ <dynamic> public ${} ]\n", n.to_string_lossy()),
    };
    Ok(Value::Str(PhpStr::from(s)))
}
fn p_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, n, _) = need_prop!(vm, a);
    Ok(Value::Str(n))
}
fn p_mangled(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    if dynamic {
        return Ok(Value::Str(n));
    }
    let Some(p) = pinfo(&c, &n) else {
        return Ok(Value::Str(n));
    };
    let m = if p.flags & mflags::PRIVATE != 0 {
        let mut v = vec![0u8];
        v.extend_from_slice(unsafe { &(*p.class).name });
        v.push(0);
        v.extend_from_slice(&n);
        PhpStr::from(v)
    } else if p.flags & mflags::PROTECTED != 0 {
        let mut v = b"\0*\0".to_vec();
        v.extend_from_slice(&n);
        PhpStr::from(v)
    } else {
        n
    };
    Ok(Value::Str(m))
}
/// Reads a property with the declaring class as scope (Reflection ignores
/// visibility).
fn with_scope<T>(vm: &mut Vm, scope: *const Class, f: impl FnOnce(&mut Vm) -> R<T>) -> R<T> {
    // Run as if in a method of the declaring class: a lightweight frame
    // swap of the scope pointer.
    let saved = vm.ex.frames.last().map(|f| (f.scope, f.static_class));
    if let Some(top) = vm.ex.frames.last_mut() {
        top.scope = scope;
    }
    let r = f(vm);
    if let (Some((s, sc)), Some(top)) = (saved, vm.ex.frames.last_mut()) {
        top.scope = s;
        top.static_class = sc;
    }
    r
}
fn p_get_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let info = pinfo(&c, &n);
    if let Some(p) = &info
        && p.is_static
    {
        vm.ensure_statics(&c)?;
        let sp = c.find_static(&n).unwrap();
        let v = sp.cell.get();
        if vm.is_uninit(&v) {
            return vm.throw(
                "Error",
                &format!(
                    "Typed static property {}::${} must not be accessed before initialization",
                    unsafe { (*sp.class).name.dname() },
                    n.dname()
                ),
            );
        }
        return Ok(v);
    }
    let Some(o) = a.obj(0) else {
        return vm.type_error(&format!(
            "ReflectionProperty::getValue(): Argument #1 ($object) must be provided for instance properties"
        ));
    };
    if !vm.class_of(&o).is_subclass_of(&c) {
        return vm.type_error(&format!(
            "Cannot fetch property {}::${} of instance of {}",
            c.name.dname(),
            n.dname(),
            vm.class_of(&o).name.dname()
        ));
    }
    let scope = info.map(|p| p.class).unwrap_or(Rc::as_ptr(&c));
    with_scope(vm, scope, |vm| vm.read_prop(&o, &n, false, None))
}
fn p_set_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let info = pinfo(&c, &n);
    if let Some(p) = &info
        && p.is_static
    {
        vm.ensure_statics(&c)?;
        let v = if a.has(1) { a.val(1) } else { a.val(0) };
        let sp = c.find_static(&n).unwrap();
        let old = sp.cell.set(v);
        drop(old);
        return Ok(Value::Null);
    }
    let Some(o) = a.obj(0) else {
        return vm.type_error("ReflectionProperty::setValue(): Argument #1 ($objectOrValue) must be of type object, null given");
    };
    let v = a.val(1);
    let scope = info.map(|p| p.class).unwrap_or(Rc::as_ptr(&c));
    with_scope(vm, scope, |vm| vm.write_prop(&o, &n, v, None))?;
    Ok(Value::Null)
}
fn p_get_raw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let info = pinfo(&c, &n);
    if let Some(p) = &info
        && let Some(slot) = p.slot
    {
        if p.flags & mflags::VIRTUAL != 0 {
            return refl_error(vm, "Must not read virtual property");
        }
        let v = o.obj().slots.borrow()[slot as usize].clone();
        if vm.is_uninit(&v) {
            return vm.throw(
                "Error",
                &format!(
                    "Typed property {}::${} must not be accessed before initialization",
                    c.name.dname(),
                    n.dname()
                ),
            );
        }
        return Ok(v.into_deref());
    }
    p_get_value(vm, a)
}
fn p_set_raw(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let Some(o) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let v = a.val(1);
    let info = pinfo(&c, &n);
    if let Some(p) = &info
        && let Some(slot) = p.slot
    {
        let v = match &p.ty {
            Some(t) => {
                let props = c.props.borrow();
                let idx = c.prop_map.borrow()[&n][0];
                let pi = props[idx as usize].clone();
                drop(props);
                vm.coerce_prop(&pi, t, v, &c)?
            }
            None => v,
        };
        let mut slots = o.obj().slots.borrow_mut();
        let old = std::mem::replace(&mut slots[slot as usize], v);
        drop(slots);
        drop(old);
        return Ok(Value::Null);
    }
    p_set_value(vm, a)
}
fn p_is_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    let info = pinfo(&c, &n);
    if let Some(p) = &info
        && p.is_static
    {
        vm.ensure_statics(&c)?;
        let v = c.find_static(&n).unwrap().cell.get();
        return Ok(Value::Bool(!vm.is_uninit(&v)));
    }
    let Some(o) = a.obj(0) else {
        return vm.type_error("ReflectionProperty::isInitialized(): Argument #1 ($object) must be provided for instance properties");
    };
    if dynamic {
        return Ok(Value::Bool(o.obj().props.borrow().get_str(&n).is_some()));
    }
    match info.and_then(|p| p.slot) {
        Some(slot) => {
            let v = o.obj().slots.borrow()[slot as usize].clone();
            Ok(Value::Bool(!vm.is_uninit(&v)))
        }
        None => Ok(Value::Bool(false)),
    }
}
fn pflag(vm: &mut Vm, a: &Args, f: u32) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    if dynamic {
        return Ok(Value::Bool(f == mflags::PUBLIC));
    }
    Ok(Value::Bool(pinfo(&c, &n).is_some_and(|p| p.flags & f != 0)))
}
fn p_is_public(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::PUBLIC)
}
fn p_is_private(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::PRIVATE)
}
fn p_is_protected(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::PROTECTED)
}
fn p_is_private_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::PRIVATE_SET)
}
fn p_is_protected_set(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    Ok(Value::Bool(
        pinfo(&c, &n).is_some_and(|p| prop_modifiers(p.flags) & 2048 != 0),
    ))
}
fn p_is_static(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    Ok(Value::Bool(pinfo(&c, &n).is_some_and(|p| p.is_static)))
}
fn p_is_readonly(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::READONLY)
}
fn p_is_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, dynamic) = need_prop!(vm, a);
    Ok(Value::Bool(!dynamic))
}
fn p_is_dynamic(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, dynamic) = need_prop!(vm, a);
    Ok(Value::Bool(dynamic))
}
fn p_is_abstract(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::ABSTRACT)
}
fn p_is_virtual(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::VIRTUAL)
}
fn p_is_promoted(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::PROMOTED)
}
fn p_is_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    pflag(vm, a, mflags::FINAL)
}
fn p_modifiers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    if dynamic {
        return Ok(Value::Int(1));
    }
    let p = pinfo(&c, &n).unwrap();
    let mut m = prop_modifiers(p.flags);
    if p.is_static {
        m |= 16;
    }
    Ok(Value::Int(m))
}
fn p_declaring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    let dc = if dynamic {
        c
    } else {
        vm.rc_class(pinfo(&c, &n).unwrap().class)
    };
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionClass",
        Refl::Class(dc),
    )?))
}
fn p_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    Ok(pinfo(&c, &n)
        .and_then(|p| p.doc)
        .map(Value::Str)
        .unwrap_or(Value::Bool(false)))
}
fn p_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    match pinfo(&c, &n).and_then(|p| p.ty.map(|t| (t, p.class))) {
        Some((t, cl)) => Ok(Value::Object(new_type(vm, t, cl)?)),
        None => Ok(Value::Null),
    }
}
fn p_has_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    Ok(Value::Bool(pinfo(&c, &n).is_some_and(|p| p.ty.is_some())))
}
fn p_has_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    if dynamic {
        return Ok(Value::Bool(false));
    }
    let p = pinfo(&c, &n).unwrap();
    Ok(Value::Bool(
        p.default.is_some() || (p.ty.is_none() && p.flags & mflags::VIRTUAL == 0),
    ))
}
fn p_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    if dynamic {
        return Ok(Value::Null);
    }
    let p = pinfo(&c, &n).unwrap();
    match p.default {
        Some(ce) => vm.eval_const_expr(&ce, p.class),
        None => Ok(Value::Null),
    }
}
fn p_attrs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let (attrs, cl) = pinfo(&c, &n)
        .map(|p| (p.attrs, p.class))
        .unwrap_or((Vec::new(), Rc::as_ptr(&c)));
    attributes_array(vm, a, &attrs, 8, cl, 0)
}
fn p_has_hooks(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    Ok(Value::Bool(
        pinfo(&c, &n).is_some_and(|p| p.has_hooks.0 || p.has_hooks.1),
    ))
}
fn hook_func(c: &Class, n: &PhpStr, get: bool) -> Option<Rc<Func>> {
    let map = c.prop_map.borrow();
    let idxs = map.get(n)?;
    let p = c.props.borrow()[idxs[0] as usize].clone();
    if get { p.get.clone() } else { p.set.clone() }
}
fn p_hooks(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let mut out = PhpArray::new();
    for (k, get) in [("get", true), ("set", false)] {
        if let Some(f) = hook_func(&c, &n, get) {
            let o = new_refl(
                vm,
                "ReflectionMethod",
                Refl::Method {
                    func: f,
                    cls: c.clone(),
                },
            )?;
            out.insert(Key::from(k), Value::Object(o));
        }
    }
    Ok(Value::Array(out))
}
fn hook_kind(vm: &mut Vm, v: &Value) -> bool {
    match v {
        Value::Object(o) => {
            let n = vm.get_prop_raw(o, b"name");
            n.as_str().is_some_and(|s| s.as_bytes() == b"Get")
        }
        _ => true,
    }
}
fn p_has_hook(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let get = hook_kind(vm, &a.val(0));
    Ok(Value::Bool(hook_func(&c, &n, get).is_some()))
}
fn p_hook(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, _) = need_prop!(vm, a);
    let get = hook_kind(vm, &a.val(0));
    match hook_func(&c, &n, get) {
        Some(f) => Ok(Value::Object(new_refl(
            vm,
            "ReflectionMethod",
            Refl::Method { func: f, cls: c },
        )?)),
        None => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------- class constants

fn this_const(a: &Args) -> Option<(Rc<Class>, PhpStr)> {
    data(a.this_obj(), |r| match r {
        Refl::Const { cls, name } => Some((cls.clone(), name.clone())),
        _ => None,
    })
}

macro_rules! need_const {
    ($vm:expr, $a:expr) => {
        match this_const($a) {
            Some(p) => p,
            None => {
                return refl_error(
                    $vm,
                    "Internal error: Failed to retrieve the reflection object",
                )
            }
        }
    };
}

fn cc_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let cls = class_arg(vm, &a.val(0))?;
    let n = a.str(1);
    let Some(k) = cls.find_const(&n) else {
        return refl_error(
            vm,
            &format!(
                "Constant {}::{} does not exist",
                cls.name.dname(),
                n.dname()
            ),
        );
    };
    let oc = vm.class_of(&o);
    if oc.lname.as_bytes().starts_with(b"reflectionenum") && k.flags & mflags::CASE == 0 {
        return refl_error(
            vm,
            &format!("Constant {}::{} is not a case", cls.name.dname(), n.dname()),
        );
    }
    if oc.lname.as_bytes() == b"reflectionenumbackedcase" && cls.enum_backing.is_none() {
        return refl_error(
            vm,
            &format!(
                "Enum case {}::{} is not a backed case",
                cls.name.dname(),
                n.dname()
            ),
        );
    }
    vm.set_prop_raw(&o, b"name", Value::Str(n.clone()));
    vm.set_prop_raw(&o, b"class", Value::Str(unsafe { (*k.class).name.clone() }));
    set_data(&o, Refl::Const { cls, name: n });
    Ok(Value::Null)
}
fn cc_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    let v = if k.flags & mflags::CASE != 0 {
        vm.enum_case(&c, &n)?
    } else {
        vm.eval_class_const(&k)?
    };
    let vis = if k.flags & mflags::PRIVATE != 0 {
        "private"
    } else if k.flags & mflags::PROTECTED != 0 {
        "protected"
    } else {
        "public"
    };
    let ty = match &v {
        Value::Object(_) => "object".to_string(),
        v => crate::builtins::var::type_name(v)
            .replace("integer", "int")
            .replace("boolean", "bool"),
    };
    let mut val = Vec::new();
    match &v {
        Value::Array(_) => val.extend_from_slice(b"Array"),
        Value::Object(_) => val.extend_from_slice(b"Object"),
        other => vm.append_to_string(&mut val, other)?,
    }
    let s = format!(
        "Constant [ {}{} {} {} ] {{ {} }}\n",
        if k.flags & mflags::FINAL != 0 {
            "final "
        } else {
            ""
        },
        vis,
        ty,
        n.to_string_lossy(),
        String::from_utf8_lossy(&val)
    );
    Ok(Value::Str(PhpStr::from(s)))
}
fn cc_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, n) = need_const!(vm, a);
    Ok(Value::Str(n))
}
fn cc_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    if k.flags & mflags::CASE != 0 {
        return vm.enum_case(&c, &n);
    }
    vm.eval_class_const(&k)
}
fn ccflag(vm: &mut Vm, a: &Args, f: u32) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    Ok(Value::Bool(
        c.find_const(&n).is_some_and(|k| k.flags & f != 0),
    ))
}
fn cc_is_public(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::PUBLIC)
}
fn cc_is_private(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::PRIVATE)
}
fn cc_is_protected(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::PROTECTED)
}
fn cc_is_final(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::FINAL)
}
fn cc_is_case(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::CASE)
}
fn cc_is_deprecated(vm: &mut Vm, a: &mut Args) -> R<Value> {
    ccflag(vm, a, mflags::DEPRECATED)
}
fn cc_modifiers(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    Ok(Value::Int(
        c.find_const(&n)
            .map(|k| const_modifiers(k.flags))
            .unwrap_or(0),
    ))
}
fn cc_declaring(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    let dc = vm.rc_class(k.class);
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionClass",
        Refl::Class(dc),
    )?))
}
fn cc_doc(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    Ok(c.find_const(&n)
        .and_then(|k| k.doc.clone())
        .map(Value::Str)
        .unwrap_or(Value::Bool(false)))
}
fn cc_attrs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    let attrs = k.attrs.to_vec();
    attributes_array(vm, a, &attrs, 16, k.class, 0)
}
fn cc_has_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    Ok(Value::Bool(
        c.find_const(&n).is_some_and(|k| k.ty.is_some()),
    ))
}
fn cc_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    match &k.ty {
        Some(t) => Ok(Value::Object(new_type(vm, t.clone(), k.class)?)),
        None => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------- parameters

fn this_param(vm: &mut Vm, a: &Args) -> Option<(Rc<Func>, Option<ObjRef>, usize, ParamInfo)> {
    let r = data(a.this_obj(), |r| match r {
        Refl::Param { func, closure, idx } => Some((func.clone(), closure.clone(), *idx)),
        _ => None,
    })?;
    let ps = vm.params_of(&r.0).to_vec();
    let p = ps.get(r.2)?.clone();
    Some((r.0, r.1, r.2, p))
}

macro_rules! need_param {
    ($vm:expr, $a:expr) => {
        match this_param($vm, $a) {
            Some(p) => p,
            None => {
                return refl_error(
                    $vm,
                    "Internal error: Failed to retrieve the reflection object",
                )
            }
        }
    };
}

fn par_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let fv = a.val(0);
    let (func, closure) = match &fv {
        Value::Str(s) => {
            if let Some(p) = s.windows(2).position(|w| w == b"::") {
                let c = class_arg(vm, &Value::Str(PhpStr::new(&s[..p])))?;
                (find_method_in(vm, &c, &s[p + 2..])?, None)
            } else {
                match vm
                    .functions
                    .get(&lower(s.strip_prefix(b"\\").unwrap_or(s)))
                    .cloned()
                {
                    Some(f) => (f, None),
                    None => {
                        return refl_error(
                            vm,
                            &format!("Function {}() does not exist", s.to_string_lossy()),
                        );
                    }
                }
            }
        }
        Value::Array(arr) if arr.len() == 2 => {
            let c = class_arg(vm, &arr.get_int(0).map(|x| x.deref()).unwrap_or_default())?;
            let m = arr.get_int(1).map(|x| x.deref()).unwrap_or_default();
            let m = vm.to_php_str(&m)?;
            (find_method_in(vm, &c, &m)?, None)
        }
        Value::Object(ob) => match vm.closure_data(ob) {
            Some(cd) => (cd.func.clone(), Some(ob.clone())),
            None => {
                let c = vm.class_of(ob);
                match c.find_method(b"__invoke") {
                    Some(f) => (f, None),
                    None => return refl_error(vm, "Method __invoke does not exist"),
                }
            }
        },
        _ => {
            return refl_error(
                vm,
                "The parameter class is expected to be either a string, an array(class, method) or a callable object",
            );
        }
    };
    let ps = vm.params_of(&func).to_vec();
    let idx = match a.val(1) {
        Value::Int(i) => {
            if i < 0 || i as usize >= ps.len() {
                return refl_error(
                    vm,
                    "The parameter specified by its offset could not be found",
                );
            }
            i as usize
        }
        v => {
            let n = vm.to_php_str(&v)?;
            match ps.iter().position(|p| p.name == n) {
                Some(i) => i,
                None => {
                    return refl_error(vm, "The parameter specified by its name could not be found");
                }
            }
        }
    };
    vm.set_prop_raw(&o, b"name", Value::Str(ps[idx].name.clone()));
    set_data(&o, Refl::Param { func, closure, idx });
    Ok(Value::Null)
}
fn par_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, i, p) = need_param!(vm, a);
    let nreq = match &f.kind {
        FuncKind::User(c) => c.num_required as usize,
        FuncKind::Native { .. } => vm.native_sig(&f).num_required as usize,
    };
    Ok(Value::Str(PhpStr::from(param_string(vm, &p, i, i < nreq))))
}
fn par_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Str(p.name))
}
fn par_by_ref(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.by_ref))
}
fn par_by_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(!p.by_ref))
}
fn par_func(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _, _) = need_param!(vm, a);
    if f.scope().is_some() && c.is_none() {
        let cls = vm.rc_class(f.scope.get());
        return Ok(Value::Object(new_refl(
            vm,
            "ReflectionMethod",
            Refl::Method { func: f, cls },
        )?));
    }
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionFunction",
        Refl::Func {
            func: f,
            closure: c,
        },
    )?))
}
fn par_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, c, _, _) = need_param!(vm, a);
    let scope = match &c {
        Some(cl) => closure_cd(cl, |d| d.scope).unwrap_or(std::ptr::null()),
        None => f.scope.get(),
    };
    if scope.is_null() {
        return Ok(Value::Null);
    }
    let cls = vm.rc_class(scope);
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionClass",
        Refl::Class(cls),
    )?))
}
fn par_get_class(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated("Method ReflectionParameter::getClass() is deprecated since 8.0, use ReflectionParameter::getType() instead")?;
    let (f, _, _, p) = need_param!(vm, a);
    let Some(t) = &p.ty else {
        return Ok(Value::Null);
    };
    if t.classes.len() != 1 || t.classes[0].len() != 1 {
        return Ok(Value::Null);
    }
    let n = t.classes[0][0].clone();
    let ln = lower(&n);
    let c = match ln.as_bytes() {
        b"self" => Some(vm.rc_class(f.scope.get())),
        b"parent" => f.scope().and_then(|c| c.parent.clone()),
        _ => vm.find_class(&n, true)?,
    };
    match c {
        Some(c) => Ok(Value::Object(new_refl(
            vm,
            "ReflectionClass",
            Refl::Class(c),
        )?)),
        None => refl_error(
            vm,
            &format!("Class \"{}\" does not exist", n.to_string_lossy()),
        ),
    }
}
fn par_has_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.ty.is_some()))
}
fn par_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _, p) = need_param!(vm, a);
    match p.ty {
        Some(t) => Ok(Value::Object(new_type(vm, t, f.scope.get())?)),
        None => Ok(Value::Null),
    }
}
fn par_is_array(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated("Method ReflectionParameter::isArray() is deprecated since 8.0, use ReflectionParameter::getType() instead")?;
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.ty.is_some_and(|t| {
        t.mask & !tmask::NULL == tmask::ARRAY && t.classes.is_empty()
    })))
}
fn par_is_callable(vm: &mut Vm, a: &mut Args) -> R<Value> {
    vm.deprecated("Method ReflectionParameter::isCallable() is deprecated since 8.0, use ReflectionParameter::getType() instead")?;
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.ty.is_some_and(|t| {
        t.mask & !tmask::NULL == tmask::CALLABLE && t.classes.is_empty()
    })))
}
fn par_allows_null(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.ty.as_ref().is_none_or(|t| t.allows_null())))
}
fn par_position(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, i, _) = need_param!(vm, a);
    Ok(Value::Int(i as i64))
}
fn par_optional(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, i, p) = need_param!(vm, a);
    let nreq = match &f.kind {
        FuncKind::User(c) => c.num_required as usize,
        FuncKind::Native { .. } => vm.native_sig(&f).num_required as usize,
    };
    Ok(Value::Bool(i >= nreq || p.variadic))
}
fn par_has_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(
        !matches!(p.default, ParamDefault::None) && p.default_text.as_deref() != Some("UNKNOWN"),
    ))
}
fn par_default(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _, p) = need_param!(vm, a);
    match &p.default {
        ParamDefault::Const(v) => Ok(v.clone()),
        ParamDefault::Lazy(ce) => vm.eval_const_expr(ce, f.scope.get()),
        ParamDefault::Code => match &p.default_ce {
            Some(ce) => vm.eval_const_expr(ce, f.scope.get()),
            None => Ok(Value::Null),
        },
        ParamDefault::None => {
            refl_error(vm, "Internal error: Failed to retrieve the default value")
        }
    }
}
fn default_const_name(p: &ParamInfo) -> Option<String> {
    let t = p.default_text.as_deref()?.trim();
    let ok = !t.is_empty()
        && t.chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '\\' || c == ':')
        && !t.chars().next().unwrap().is_ascii_digit()
        && !matches!(t.to_ascii_lowercase().as_str(), "null" | "true" | "false");
    ok.then(|| t.to_string())
}
fn par_default_is_const(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(default_const_name(&p).is_some()))
}
fn par_default_const_name(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(default_const_name(&p)
        .map(|s| Value::Str(PhpStr::from(s)))
        .unwrap_or_default())
}
fn par_variadic(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.variadic))
}
fn par_promoted(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (_, _, _, p) = need_param!(vm, a);
    Ok(Value::Bool(p.promote.is_some()))
}
fn par_attrs(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (f, _, _, p) = need_param!(vm, a);
    let attrs = p.attrs.to_vec();
    attributes_array(vm, a, &attrs, 32, f.scope.get(), 0)
}

// ---------------------------------------------------------------- types

fn builtin_names(mask: u32) -> Vec<&'static str> {
    let mut v = Vec::new();
    let m = mask;
    if m & tmask::MIXED != 0 {
        v.push("mixed");
    }
    if m & tmask::STATIC != 0 {
        v.push("static");
    }
    if m & tmask::CALLABLE != 0 {
        v.push("callable");
    }
    if m & tmask::ITERABLE != 0 {
        v.push("iterable");
    }
    if m & tmask::OBJECT != 0 {
        v.push("object");
    }
    if m & tmask::ARRAY != 0 {
        v.push("array");
    }
    if m & tmask::STRING != 0 {
        v.push("string");
    }
    if m & tmask::INT != 0 {
        v.push("int");
    }
    if m & tmask::FLOAT != 0 {
        v.push("float");
    }
    if m & tmask::BOOL == tmask::BOOL {
        v.push("bool");
    } else if m & tmask::FALSE != 0 {
        v.push("false");
    } else if m & tmask::TRUE != 0 {
        v.push("true");
    }
    if m & tmask::VOID != 0 {
        v.push("void");
    }
    if m & tmask::NEVER != 0 {
        v.push("never");
    }
    v
}

/// The ReflectionType object for a type declaration.
fn new_type(vm: &mut Vm, t: TypeHint, scope: *const Class) -> R<ObjRef> {
    let mut parts = builtin_names(t.mask).len() + t.classes.len();
    let null = t.mask & tmask::NULL != 0 && t.mask & tmask::MIXED == 0;
    if t.mask & tmask::ITERABLE != 0 {
        // iterable is Traversable|array since PHP 8.2.
        parts += 1;
    }
    if t.classes.len() == 1 && t.classes[0].len() > 1 && t.mask == 0 {
        return new_refl(
            vm,
            "ReflectionIntersectionType",
            Refl::Type {
                ty: t,
                single: None,
                scope,
            },
        );
    }
    if parts <= 1 {
        let name: PhpStr = if let Some(c) = t.classes.first() {
            c[0].clone()
        } else if let Some(n) = builtin_names(t.mask).first() {
            PhpStr::from(*n)
        } else {
            PhpStr::from("null")
        };
        let builtin = t.classes.is_empty();
        return new_refl(
            vm,
            "ReflectionNamedType",
            Refl::Type {
                ty: t,
                single: Some((name, builtin)),
                scope,
            },
        );
    }
    let _ = null;
    new_refl(
        vm,
        "ReflectionUnionType",
        Refl::Type {
            ty: t,
            single: None,
            scope,
        },
    )
}

fn this_type(a: &Args) -> Option<(TypeHint, Option<(PhpStr, bool)>, *const Class)> {
    data(a.this_obj(), |r| match r {
        Refl::Type { ty, single, scope } => Some((ty.clone(), single.clone(), *scope)),
        _ => None,
    })
}

fn t_allows_null(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((t, _, _)) = this_type(a) else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(t.allows_null()))
}
fn t_to_string(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((t, _, _)) = this_type(a) else {
        return Ok(Value::Str(PhpStr::empty()));
    };
    if t.mask & tmask::ITERABLE != 0
        && t.classes.is_empty()
        && t.mask & !(tmask::ITERABLE | tmask::NULL) == 0
    {
        return Ok(Value::Str(PhpStr::from(if t.mask & tmask::NULL != 0 {
            "?iterable"
        } else {
            "iterable"
        })));
    }
    Ok(Value::Str(PhpStr::from(t.display())))
}
fn t_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((_, single, _)) = this_type(a) else {
        return Ok(Value::Str(PhpStr::empty()));
    };
    Ok(Value::Str(single.map(|s| s.0).unwrap_or_default()))
}
fn t_builtin(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((t, single, _)) = this_type(a) else {
        return Ok(Value::Bool(false));
    };
    let _ = t;
    Ok(Value::Bool(
        single.is_some_and(|s| s.1 && s.0.as_bytes() != b"static"),
    ))
}
fn t_types(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((t, _, scope)) = this_type(a) else {
        return Ok(Value::Array(PhpArray::new()));
    };
    let mut out = PhpArray::new();
    let is_inter = vm.class_of(a.this_obj()).lname.as_bytes() == b"reflectionintersectiontype";
    if is_inter {
        for n in &t.classes[0] {
            let o = new_refl(
                vm,
                "ReflectionNamedType",
                Refl::Type {
                    ty: TypeHint {
                        mask: 0,
                        classes: vec![vec![n.clone()]],
                    },
                    single: Some((n.clone(), false)),
                    scope,
                },
            )?;
            let _ = out.push(Value::Object(o));
        }
        return Ok(Value::Array(out));
    }
    for c in &t.classes {
        let o = if c.len() > 1 {
            new_refl(
                vm,
                "ReflectionIntersectionType",
                Refl::Type {
                    ty: TypeHint {
                        mask: 0,
                        classes: vec![c.clone()],
                    },
                    single: None,
                    scope,
                },
            )?
        } else {
            new_refl(
                vm,
                "ReflectionNamedType",
                Refl::Type {
                    ty: TypeHint {
                        mask: 0,
                        classes: vec![c.clone()],
                    },
                    single: Some((c[0].clone(), false)),
                    scope,
                },
            )?
        };
        let _ = out.push(Value::Object(o));
    }
    let mut names = builtin_names(t.mask);
    if t.mask & tmask::ITERABLE != 0 {
        names.retain(|n| *n != "iterable");
        let o = new_refl(
            vm,
            "ReflectionNamedType",
            Refl::Type {
                ty: TypeHint {
                    mask: 0,
                    classes: vec![vec![PhpStr::from("Traversable")]],
                },
                single: Some((PhpStr::from("Traversable"), false)),
                scope,
            },
        )?;
        let _ = out.push(Value::Object(o));
        if !names.contains(&"array") {
            names.push("array");
        }
    }
    if t.mask & tmask::NULL != 0 && t.mask & tmask::MIXED == 0 {
        names.push("null");
    }
    for n in names {
        let bit = match n {
            "int" => tmask::INT,
            "string" => tmask::STRING,
            "float" => tmask::FLOAT,
            "bool" => tmask::BOOL,
            "array" => tmask::ARRAY,
            "null" => tmask::NULL,
            "false" => tmask::FALSE,
            "true" => tmask::TRUE,
            "object" => tmask::OBJECT,
            "callable" => tmask::CALLABLE,
            "static" => tmask::STATIC,
            _ => 0,
        };
        let o = new_refl(
            vm,
            "ReflectionNamedType",
            Refl::Type {
                ty: TypeHint {
                    mask: bit,
                    classes: Vec::new(),
                },
                single: Some((PhpStr::from(n), true)),
                scope,
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}

// ---------------------------------------------------------------- attributes

/// `getAttributes()`: the attributes, filtered by name (exactly or with
/// IS_INSTANCEOF).
fn attributes_array(
    vm: &mut Vm,
    a: &Args,
    attrs: &[AttrInfo],
    target: i64,
    scope: *const Class,
    _base: usize,
) -> R<Value> {
    let filter = a.opt_str(0);
    let flags = a.int_or(1, 0);
    if flags & !2 != 0 {
        return vm.value_error("ReflectionFunctionAbstract::getAttributes(): Argument #2 ($flags) must be a valid attribute filter flag");
    }
    let filter_cls = match (&filter, flags & 2 != 0) {
        (Some(n), true) => match vm.find_class(n, true)? {
            Some(c) => Some(c),
            None => {
                return vm.throw(
                    "Error",
                    &format!("Class \"{}\" not found", n.to_string_lossy()),
                );
            }
        },
        _ => None,
    };
    let mut out = PhpArray::new();
    for info in attrs {
        let name = info.name.strip_prefix(b"\\").unwrap_or(&info.name);
        if let Some(f) = &filter {
            if let Some(fc) = &filter_cls {
                match vm.find_class(name, true)? {
                    Some(c) if c.is_subclass_of(fc) => {}
                    _ => continue,
                }
            } else if !lower(name).eq(&lower(f.strip_prefix(b"\\").unwrap_or(f))) {
                continue;
            }
        }
        let repeated = attrs
            .iter()
            .filter(|x| lower(&x.name) == lower(&info.name))
            .count()
            > 1;
        let o = new_refl(
            vm,
            "ReflectionAttribute",
            Refl::Attr {
                info: info.clone(),
                target,
                scope,
                repeated,
            },
        )?;
        let _ = out.push(Value::Object(o));
    }
    Ok(Value::Array(out))
}

fn this_attr(a: &Args) -> Option<(AttrInfo, i64, *const Class, bool)> {
    data(a.this_obj(), |r| match r {
        Refl::Attr {
            info,
            target,
            scope,
            repeated,
        } => Some((info.clone(), *target, *scope, *repeated)),
        _ => None,
    })
}
fn a_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((info, _, _, _)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    Ok(Value::Str(PhpStr::new(
        info.name.strip_prefix(b"\\").unwrap_or(&info.name),
    )))
}
fn a_target(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((_, t, _, _)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    Ok(Value::Int(t))
}
fn a_repeated(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((_, _, _, r)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    Ok(Value::Bool(r))
}
fn attr_args(vm: &mut Vm, info: &AttrInfo, scope: *const Class) -> R<PhpArray> {
    let mut out = PhpArray::new();
    for (n, ce) in &info.args {
        let v = vm.eval_const_expr(ce, scope)?;
        match n {
            Some(n) => {
                if out.get_str(n).is_some() {
                    return vm.throw(
                        "Error",
                        &format!(
                            "Named parameter ${} overwrites previous argument",
                            n.to_string_lossy()
                        ),
                    );
                }
                out.insert(Key::Str(n.clone()), v);
            }
            None => {
                let _ = out.push(v);
            }
        }
    }
    Ok(out)
}
fn a_args(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((info, _, scope, _)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    Ok(Value::Array(attr_args(vm, &info, scope)?))
}
fn a_new(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((info, target, scope, repeated)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    let name = PhpStr::new(info.name.strip_prefix(b"\\").unwrap_or(&info.name));
    let Some(cls) = vm.find_class(&name, true)? else {
        return vm.throw(
            "Error",
            &format!("Attribute class \"{}\" not found", name.to_string_lossy()),
        );
    };
    // The class must be an attribute (#[Attribute]) allowing this target.
    let attr_decl = cls
        .attrs
        .iter()
        .find(|x| lower(x.name.strip_prefix(b"\\").unwrap_or(&x.name)).as_bytes() == b"attribute")
        .cloned();
    let Some(attr_decl) = attr_decl else {
        return vm.throw(
            "Error",
            &format!(
                "Attempting to use non-attribute class \"{}\" as attribute",
                cls.name.dname()
            ),
        );
    };
    let flags = match attr_decl.args.first() {
        Some((_, ce)) => match vm.eval_const_expr(ce, Rc::as_ptr(&cls))? {
            Value::Int(i) => i,
            _ => 127,
        },
        None => 127,
    };
    if flags & target == 0 {
        let tn = target_name(target);
        return vm.throw(
            "Error",
            &format!(
                "Attribute \"{}\" cannot target {} (allowed targets: {})",
                cls.name.dname(),
                tn,
                allowed_targets(flags)
            ),
        );
    }
    if repeated && flags & 128 == 0 {
        return vm.throw(
            "Error",
            &format!("Attribute \"{}\" must not be repeated", cls.name.dname()),
        );
    }
    let args = attr_args(vm, &info, scope)?;
    new_with_args(vm, &cls, args)
}
fn target_name(t: i64) -> &'static str {
    match t {
        1 => "class",
        2 => "function",
        4 => "method",
        8 => "property",
        16 => "class constant",
        32 => "parameter",
        64 => "constant",
        _ => "unknown",
    }
}
fn allowed_targets(f: i64) -> String {
    let mut v = Vec::new();
    for (bit, n) in [
        (1, "class"),
        (2, "function"),
        (4, "method"),
        (8, "property"),
        (16, "class constant"),
        (32, "parameter"),
        (64, "constant"),
    ] {
        if f & bit != 0 {
            v.push(n);
        }
    }
    v.join(", ")
}
fn a_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some((info, _, scope, _)) = this_attr(a) else {
        return Ok(Value::Null);
    };
    let args = attr_args(vm, &info, scope)?;
    let mut s = format!(
        "Attribute [ {} ]",
        info.name
            .strip_prefix(b"\\")
            .map(String::from_utf8_lossy)
            .unwrap_or(String::from_utf8_lossy(&info.name))
    );
    if !args.is_empty() {
        s.push_str(" {\n");
        s.push_str(&format!("  - Arguments [{}] {{\n", args.len()));
        for (i, (k, v)) in args.iter().enumerate() {
            let mut out = Vec::new();
            let _ = vm.var_export_into(&mut out, &v.clone(), 0);
            let key = match k {
                goro_runtime::KeyRef::Str(s) => format!("{} = ", s.to_string_lossy()),
                _ => String::new(),
            };
            s.push_str(&format!(
                "    Argument #{} [ {}{} ]\n",
                i,
                key,
                String::from_utf8_lossy(&out)
            ));
        }
        s.push_str("  }\n}");
    }
    s.push('\n');
    Ok(Value::Str(PhpStr::from(s)))
}

// ---------------------------------------------------------------- enums

fn e_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let cls = class_arg(vm, &a.val(0))?;
    if !cls.is_enum() {
        return refl_error(
            vm,
            &format!("Class \"{}\" is not an enum", cls.name.dname()),
        );
    }
    vm.set_prop_raw(&o, b"name", Value::Str(cls.name.clone()));
    set_data(&o, Refl::Class(cls));
    Ok(Value::Null)
}
fn e_has_case(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    Ok(Value::Bool(
        c.find_const(&n)
            .is_some_and(|k| k.flags & mflags::CASE != 0),
    ))
}
fn e_get_case(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let n = a.str(0);
    match c.find_const(&n) {
        Some(k) if k.flags & mflags::CASE != 0 => {
            let cls = if c.enum_backing.is_some() {
                "ReflectionEnumBackedCase"
            } else {
                "ReflectionEnumUnitCase"
            };
            Ok(Value::Object(new_refl(
                vm,
                cls,
                Refl::Const { cls: c, name: n },
            )?))
        }
        Some(_) => refl_error(
            vm,
            &format!("{}::{} is not a case", c.name.dname(), n.dname()),
        ),
        None => refl_error(
            vm,
            &format!("Case {}::{} does not exist", c.name.dname(), n.dname()),
        ),
    }
}
fn e_cases(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let consts = c.consts.borrow().clone();
    let mut out = PhpArray::new();
    let cls = if c.enum_backing.is_some() {
        "ReflectionEnumBackedCase"
    } else {
        "ReflectionEnumUnitCase"
    };
    for k in consts {
        if k.flags & mflags::CASE != 0 {
            let o = new_refl(
                vm,
                cls,
                Refl::Const {
                    cls: c.clone(),
                    name: k.name.clone(),
                },
            )?;
            let _ = out.push(Value::Object(o));
        }
    }
    Ok(Value::Array(out))
}
fn e_is_backed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    Ok(Value::Bool(c.enum_backing.is_some()))
}
fn e_backing_type(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    match c.enum_backing {
        Some(m) => Ok(Value::Object(new_type(
            vm,
            TypeHint {
                mask: m,
                classes: Vec::new(),
            },
            Rc::as_ptr(&c),
        )?)),
        None => Ok(Value::Null),
    }
}
fn ec_enum(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, _) = need_const!(vm, a);
    Ok(Value::Object(new_refl(
        vm,
        "ReflectionEnum",
        Refl::Class(c),
    )?))
}
fn ec_backing(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n) = need_const!(vm, a);
    let k = c.find_const(&n).unwrap();
    let v = k.value.borrow().clone();
    match v {
        ConstState::Value(v) => Ok(v),
        ConstState::Pending(ce) => vm.eval_const_expr(&ce, Rc::as_ptr(&c)),
        ConstState::Evaluating => Ok(Value::Null),
    }
}

// ---------------------------------------------------------------- misc

fn rr_from(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let arr = a.array(0);
    let k = a.val(1);
    let key = vm.to_key(&k, "array")?;
    match arr.get(key.as_ref()) {
        None => refl_error(vm, "Array key not found"),
        Some(Value::Ref(r)) if r.refcount() > 1 || true => {
            let id = r.addr();
            let cls = vm.find_class(b"ReflectionReference", false)?.unwrap();
            let o = vm.instantiate_raw(&cls)?;
            set_data(&o, Refl::Reference(id));
            Ok(Value::Object(o))
        }
        Some(_) => Ok(Value::Null),
    }
}
fn rr_id(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let id = data(a.this_obj(), |r| match r {
        Refl::Reference(i) => *i,
        _ => 0,
    });
    // A stable opaque id (PHP returns a 20-byte sha1-like string).
    let digest = goro_ext_standard::encoding::sha1(format!("{:x}", id).as_bytes(), true);
    Ok(Value::Str(PhpStr::from(digest)))
}
fn ext_name_of(a: &Args) -> PhpStr {
    data(a.this_obj(), |r| match r {
        Refl::Extension(n) => n.clone(),
        _ => PhpStr::empty(),
    })
}
fn ext_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    let m = vm
        .modules
        .iter()
        .find(|m| m.name.as_bytes().eq_ignore_ascii_case(&n))
        .map(|m| m.name);
    match m {
        Some(name) => {
            vm.set_prop_raw(&o, b"name", Value::from(name));
            set_data(&o, Refl::Extension(PhpStr::from(name)));
            Ok(Value::Null)
        }
        None => refl_error(
            vm,
            &format!("Extension \"{}\" does not exist", n.to_string_lossy()),
        ),
    }
}
fn ext_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(ext_name_of(a)))
}
fn ext_version(_vm: &mut Vm, _a: &mut Args) -> R<Value> {
    Ok(Value::from("8.5.4"))
}
fn ext_functions(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = ext_name_of(a);
    let mut out = PhpArray::new();
    let mods: Vec<_> = vm
        .modules
        .iter()
        .filter(|m| m.name.as_bytes() == n.as_bytes())
        .copied()
        .collect();
    for m in mods {
        for (sig, _) in m.functions {
            let name = crate::registry::sig_name(sig);
            if let Some(f) = vm.functions.get(&lower(name.as_bytes())).cloned() {
                let o = new_refl(
                    vm,
                    "ReflectionFunction",
                    Refl::Func {
                        func: f,
                        closure: None,
                    },
                )?;
                out.insert(Key::from(name), Value::Object(o));
            }
        }
    }
    Ok(Value::Array(out))
}
fn ext_constants(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = ext_name_of(a);
    let mut out = PhpArray::new();
    for m in vm
        .modules
        .iter()
        .filter(|m| m.name.as_bytes() == n.as_bytes())
    {
        for (k, v) in m.constants {
            out.insert(Key::from(*k), v.to_value());
        }
    }
    Ok(Value::Array(out))
}
fn ext_ini(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = ext_name_of(a);
    let mut out = PhpArray::new();
    let mods: Vec<_> = vm
        .modules
        .iter()
        .filter(|m| m.name.as_bytes() == n.as_bytes())
        .copied()
        .collect();
    for m in mods {
        for (k, _) in m.ini {
            let v = vm.get_ini(k.as_bytes()).map(Value::Str).unwrap_or_default();
            out.insert(Key::from(*k), v);
        }
    }
    Ok(Value::Array(out))
}
fn ext_classes(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = ext_name_of(a);
    let mut out = PhpArray::new();
    let mods: Vec<_> = vm
        .modules
        .iter()
        .filter(|m| m.name.as_bytes() == n.as_bytes())
        .copied()
        .collect();
    for m in mods {
        for (l, _) in m.classes {
            if let Some(c) = vm.find_class(l.as_bytes(), false)? {
                let o = new_refl(vm, "ReflectionClass", Refl::Class(c.clone()))?;
                out.insert(Key::Str(c.name.clone()), Value::Object(o));
            }
        }
    }
    Ok(Value::Array(out))
}
fn ext_class_names(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = ext_name_of(a);
    let mut out = PhpArray::new();
    let mods: Vec<_> = vm
        .modules
        .iter()
        .filter(|m| m.name.as_bytes() == n.as_bytes())
        .copied()
        .collect();
    for m in mods {
        for (l, _) in m.classes {
            if let Some(c) = vm.find_class(l.as_bytes(), false)? {
                let _ = out.push(Value::Str(c.name.clone()));
            }
        }
    }
    Ok(Value::Array(out))
}
fn gc_name_of(a: &Args) -> PhpStr {
    data(a.this_obj(), |r| match r {
        Refl::GlobalConst(n) => n.clone(),
        _ => PhpStr::empty(),
    })
}
fn gc_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let n = a.str(0);
    let n = PhpStr::new(n.strip_prefix(b"\\").unwrap_or(&n));
    if vm.lookup_constant(&n)?.is_none() {
        return refl_error(
            vm,
            &format!("Constant \"{}\" does not exist", n.to_string_lossy()),
        );
    }
    vm.set_prop_raw(&o, b"name", Value::Str(n.clone()));
    set_data(&o, Refl::GlobalConst(n));
    Ok(Value::Null)
}
fn gc_name(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(Value::Str(gc_name_of(a)))
}
fn gc_ns(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = gc_name_of(a);
    Ok(Value::Str(PhpStr::new(split_ns(&n).0)))
}
fn gc_short(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = gc_name_of(a);
    Ok(Value::Str(PhpStr::new(split_ns(&n).1)))
}
fn gc_value(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = gc_name_of(a);
    Ok(vm.lookup_constant(&n)?.unwrap_or_default())
}
fn gc_to_string(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let n = gc_name_of(a);
    let v = vm.lookup_constant(&n)?.unwrap_or_default();
    let ty = crate::builtins::var::type_name(&v)
        .replace("integer", "int")
        .replace("boolean", "bool");
    let mut val = Vec::new();
    vm.append_to_string(&mut val, &v)?;
    Ok(Value::Str(PhpStr::from(format!(
        "Constant [ {} {} ] {{ {} }}\n",
        ty,
        n.to_string_lossy(),
        String::from_utf8_lossy(&val)
    ))))
}
fn gen_of(a: &Args) -> Option<ObjRef> {
    data(a.this_obj(), |r| match r {
        Refl::Generator(g) => Some(g.clone()),
        _ => None,
    })
}
fn g_construct(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let o = a.this_obj().clone();
    let Some(g) = a.obj(0) else {
        return Ok(Value::Null);
    };
    let done = vm
        .with_gen(&g, |d| d.state == crate::vm::iter::GenState::Done)
        .unwrap_or(true);
    if done {
        return refl_error(
            vm,
            "Cannot create ReflectionGenerator based on a terminated Generator",
        );
    }
    set_data(&o, Refl::Generator(g));
    Ok(Value::Null)
}
fn g_line(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(g) = gen_of(a) else {
        return Ok(Value::Null);
    };
    let l = vm
        .with_gen(&g, |d| {
            d.frame
                .as_ref()
                .map(|f| f.code().line_of(f.ip.saturating_sub(1) as usize))
        })
        .flatten()
        .unwrap_or(0);
    Ok(Value::Int(l as i64))
}
fn g_file(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(g) = gen_of(a) else {
        return Ok(Value::Null);
    };
    let f = vm
        .with_gen(&g, |d| d.frame.as_ref().map(|f| f.code().file.clone()))
        .flatten()
        .unwrap_or_default();
    Ok(Value::Str(f))
}
fn g_function(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(g) = gen_of(a) else {
        return Ok(Value::Null);
    };
    let info = vm
        .with_gen(&g, |d| {
            d.frame
                .as_ref()
                .map(|f| (f.func.clone(), f.closure.clone()))
        })
        .flatten();
    match info {
        Some((f, c)) => {
            if f.scope().is_some() && c.is_none() {
                let cls = vm.rc_class(f.scope.get());
                return Ok(Value::Object(new_refl(
                    vm,
                    "ReflectionMethod",
                    Refl::Method { func: f, cls },
                )?));
            }
            Ok(Value::Object(new_refl(
                vm,
                "ReflectionFunction",
                Refl::Func {
                    func: f,
                    closure: c,
                },
            )?))
        }
        None => Ok(Value::Null),
    }
}
fn g_this(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(g) = gen_of(a) else {
        return Ok(Value::Null);
    };
    Ok(vm
        .with_gen(&g, |d| d.frame.as_ref().and_then(|f| f.this.clone()))
        .flatten()
        .map(Value::Object)
        .unwrap_or_default())
}
fn g_gen(_vm: &mut Vm, a: &mut Args) -> R<Value> {
    Ok(gen_of(a).map(Value::Object).unwrap_or_default())
}
fn g_closed(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let Some(g) = gen_of(a) else {
        return Ok(Value::Bool(true));
    };
    Ok(Value::Bool(
        vm.with_gen(&g, |d| d.state == crate::vm::iter::GenState::Done)
            .unwrap_or(true),
    ))
}

#[allow(dead_code)]
fn unused(_: Unwind) {}

// ---------------------------------------------------------------- lazy objects

fn lazy_class_check(vm: &mut Vm, c: &Rc<Class>) -> R<()> {
    if c.lname.as_bytes() == b"stdclass" {
        return Ok(());
    }
    if c.has_flag(cflags::INTERNAL) {
        return vm.throw(
            "Error",
            &format!("Cannot make instance of internal class lazy: {} is internal", c.name.dname()),
        );
    }
    let mut p = c.parent.clone();
    while let Some(k) = p {
        if k.has_flag(cflags::INTERNAL) {
            if k.lname.as_bytes() == b"stdclass" {
                return Ok(());
            }
            return vm.throw(
                "Error",
                &format!(
                    "Cannot make instance of internal class lazy: {} inherits internal class {}",
                    c.name.dname(),
                    k.name.dname()
                ),
            );
        }
        p = k.parent.clone();
    }
    Ok(())
}

fn lazy_options(vm: &mut Vm, method: &str, argn: usize, opts: i64, allow_skip_dtor: bool) -> R<()> {
    use crate::vm::lazy::{SKIP_DESTRUCTOR, SKIP_INITIALIZATION_ON_SERIALIZE};
    let pname = "options";
    if opts & !(SKIP_DESTRUCTOR | SKIP_INITIALIZATION_ON_SERIALIZE) != 0 {
        return refl_error(
            vm,
            &format!("ReflectionClass::{}(): Argument #{} (${}) contains invalid flags", method, argn, pname),
        );
    }
    if !allow_skip_dtor && opts & SKIP_DESTRUCTOR != 0 {
        return refl_error(
            vm,
            &format!(
                "ReflectionClass::{}(): Argument #{} (${}) does not accept ReflectionClass::SKIP_DESTRUCTOR",
                method, argn, pname
            ),
        );
    }
    Ok(())
}

fn new_lazy(vm: &mut Vm, a: &mut Args, proxy: bool) -> R<Value> {
    let c = need_class!(vm, a);
    let method = if proxy { "newLazyProxy" } else { "newLazyGhost" };
    let opts = a.int_or(1, 0);
    lazy_options(vm, method, 2, opts, false)?;
    lazy_class_check(vm, &c)?;
    let o = vm.instantiate(&c)?;
    let init = a.val(0);
    vm.make_lazy(&o, proxy, init, opts);
    Ok(Value::Object(o))
}
fn c_new_lazy_ghost(vm: &mut Vm, a: &mut Args) -> R<Value> {
    new_lazy(vm, a, false)
}
fn c_new_lazy_proxy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    new_lazy(vm, a, true)
}

fn obj_of_class(vm: &mut Vm, a: &Args, c: &Rc<Class>, method: &str) -> R<ObjRef> {
    let Some(o) = a.obj(0) else { return vm.type_error("object expected") };
    if !vm.class_of(&o).is_subclass_of(c) {
        return vm.type_error(&format!(
            "ReflectionClass::{}(): Argument #1 ($object) must be of type {}, {} given",
            method,
            c.name.dname(),
            vm.class_of(&o).name.dname()
        ));
    }
    Ok(o)
}

fn reset_lazy(vm: &mut Vm, a: &mut Args, proxy: bool) -> R<Value> {
    let c = need_class!(vm, a);
    let method = if proxy { "resetAsLazyProxy" } else { "resetAsLazyGhost" };
    let opts = a.int_or(2, 0);
    lazy_options(vm, method, 3, opts, true)?;
    let o = obj_of_class(vm, a, &c, method)?;
    let oc = vm.class_of(&o);
    lazy_class_check(vm, &oc)?;
    // (A ghost being initialized has its lazy flag off.)
    if o.is_lazy() || crate::vm::lazy::with_lazy(&o, |d| d.initializing).unwrap_or(false) {
        let (init, uninit) = crate::vm::lazy::with_lazy(&o, |d| (d.initializing, d.instance.is_none())).unwrap_or((false, false));
        if init {
            return vm.throw("Error", "Can not reset an object while it is being initialized");
        }
        if uninit {
            return refl_error(vm, "Object is already lazy");
        }
    }
    // The destructor runs first unless skipped.
    if opts & crate::vm::lazy::SKIP_DESTRUCTOR == 0 && !o.destructor_called() {
        let d = oc.magic.borrow().destruct.clone();
        if let Some(f) = d {
            o.mark_destructor_called();
            vm.call_func_this(&f, Some(o.clone()), Vec::new())?;
        }
    }
    // Initialized readonly properties of final classes keep their value.
    let keep: Vec<(usize, Value)> = if oc.has_flag(cflags::FINAL) {
        let props = oc.props.borrow().clone();
        let slots = o.obj().slots.borrow();
        props
            .iter()
            .filter(|p| p.flags & mflags::READONLY != 0)
            .filter_map(|p| {
                let v = slots[p.slot as usize].clone();
                (!vm.is_uninit(&v)).then_some((p.slot as usize, v))
            })
            .collect()
    } else {
        Vec::new()
    };
    let init = a.val(1);
    vm.make_lazy(&o, proxy, init, opts);
    for (slot, v) in keep {
        o.obj().slots.borrow_mut()[slot] = v;
        vm.lazy_clear_slot(&o, slot);
    }
    Ok(Value::Null)
}
fn c_reset_lazy_ghost(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset_lazy(vm, a, false)
}
fn c_reset_lazy_proxy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    reset_lazy(vm, a, true)
}
fn c_init_lazy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let o = obj_of_class(vm, a, &c, "initializeLazyObject")?;
    if !o.is_lazy() {
        return Ok(Value::Object(o));
    }
    Ok(Value::Object(vm.lazy_init(&o)?))
}
fn c_is_uninit_lazy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let o = obj_of_class(vm, a, &c, "isUninitializedLazyObject")?;
    Ok(Value::Bool(vm.is_lazy_uninit(&o)))
}
fn c_mark_lazy_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let o = obj_of_class(vm, a, &c, "markLazyObjectAsInitialized")?;
    if !vm.is_lazy_uninit(&o) {
        return Ok(Value::Object(o));
    }
    let oc = vm.class_of(&o);
    let defaults = vm.class_defaults(&oc)?;
    let lazy = crate::vm::lazy::with_lazy(&o, |d| d.lazy_slots.clone()).unwrap_or_default();
    {
        let mut slots = o.obj().slots.borrow_mut();
        for (i, l) in lazy.iter().enumerate() {
            if *l && i < slots.len() && i < defaults.len() {
                slots[i] = defaults[i].clone();
            }
        }
    }
    *o.obj().native.borrow_mut() = None;
    o.set_lazy(false);
    o.clear_destructor_called();
    Ok(Value::Object(o))
}
fn c_get_lazy_init(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let c = need_class!(vm, a);
    let o = obj_of_class(vm, a, &c, "getLazyInitializer")?;
    if !vm.is_lazy_uninit(&o) {
        return Ok(Value::Null);
    }
    Ok(crate::vm::lazy::with_lazy(&o, |d| d.init.clone()).unwrap_or_default())
}

fn lazy_prop_slot(vm: &mut Vm, a: &Args, method: &str) -> R<(ObjRef, Rc<Class>, Rc<crate::class::PropInfo>)> {
    let (c, n, dynamic) = need_prop!(vm, a);
    let Some(o) = a.obj(0) else { return vm.type_error("object expected") };
    let info = pinfo(&c, &n);
    let decl = info.as_ref().map(|p| p.class).unwrap_or(Rc::as_ptr(&c));
    let dn = unsafe { (*decl).name.dname() };
    if dynamic || info.is_none() {
        return refl_error(vm, &format!("Can not use {} on dynamic property {}::${}", method, dn, n.dname()));
    }
    let info = info.unwrap();
    if info.is_static {
        return refl_error(vm, &format!("Can not use {} on static property {}::${}", method, dn, n.dname()));
    }
    if info.flags & mflags::VIRTUAL != 0 {
        return refl_error(vm, &format!("Can not use {} on virtual property {}::${}", method, dn, n.dname()));
    }
    let oc = vm.class_of(&o);
    if !oc.is_subclass_of(&c) {
        return vm.type_error(&format!(
            "ReflectionProperty::{}(): Argument #1 ($object) must be of type {}, {} given",
            method,
            c.name.dname(),
            oc.name.dname()
        ));
    }
    let idx = c.prop_map.borrow()[&n][0];
    let p = c.props.borrow()[idx as usize].clone();
    Ok((o, c, p))
}

fn p_set_raw_lazy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, c, p) = lazy_prop_slot(vm, a, "setRawValueWithoutLazyInitialization")?;
    let target = if o.is_lazy() && !vm.is_lazy_uninit(&o) {
        // An initialized proxy: the real instance.
        crate::vm::lazy::with_lazy(&o, |d| d.instance.clone()).flatten().unwrap_or(o.clone())
    } else {
        o.clone()
    };
    let v = a.val(1);
    let v = match &p.ty {
        Some(t) => vm.coerce_prop(&p, t, v, &c)?,
        None => v,
    };
    let old = std::mem::replace(&mut target.obj().slots.borrow_mut()[p.slot as usize], v);
    drop(old);
    if vm.lazy_slot(&target, p.slot as usize) {
        vm.lazy_clear_slot(&target, p.slot as usize);
    }
    Ok(Value::Null)
}

fn p_skip_lazy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (o, c, p) = lazy_prop_slot(vm, a, "skipLazyInitialization")?;
    if !vm.lazy_slot(&o, p.slot as usize) {
        return Ok(Value::Null);
    }
    let oc = vm.class_of(&o);
    let defaults = vm.class_defaults(&oc)?;
    let _ = c;
    let v = defaults.get(p.slot as usize).cloned().unwrap_or_default();
    let old = std::mem::replace(&mut o.obj().slots.borrow_mut()[p.slot as usize], v);
    drop(old);
    vm.lazy_clear_slot(&o, p.slot as usize);
    Ok(Value::Null)
}

fn p_is_lazy(vm: &mut Vm, a: &mut Args) -> R<Value> {
    let (c, n, dynamic) = need_prop!(vm, a);
    let Some(o) = a.obj(0) else { return Ok(Value::Bool(false)) };
    if dynamic {
        return Ok(Value::Bool(false));
    }
    let Some(info) = pinfo(&c, &n) else { return Ok(Value::Bool(false)) };
    let Some(slot) = info.slot else { return Ok(Value::Bool(false)) };
    Ok(Value::Bool(vm.lazy_slot(&o, slot as usize)))
}
