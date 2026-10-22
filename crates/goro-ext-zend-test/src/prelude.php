<?php
// The classes of php-src's ext/zend_test (test.stub.php, iterators.stub.php,
// object_handlers.stub.php), declared from PHP source when the extension is
// loaded and then turned into internal classes (see lib.rs): no declaring
// file or lines, methods carrying their stub arginfo. Method bodies follow
// the C implementations; behaviour that needs object handlers (casts,
// dimension handlers...) is attached natively.
//
// Only the doc comments php-src exposes (@genstubs-expose-comment-block) are
// kept, as gen_stub.php writes them.

namespace {

#[\Deprecated(message: "use something else", since: "version 1.5")]
const ZEND_TEST_ATTRIBUTED_CONSTANT = 42;

interface _ZendTestInterface
{
    /**
         * "Lorem ipsum"
         * @see https://www.php.net
         * @since 8.2
         */
    public const DUMMY = 0;
}

class _ZendTestClass implements _ZendTestInterface {
    public const mixed TYPED_CLASS_CONST1 = [];
    public const int|array TYPED_CLASS_CONST2 = 42;
    public const int|string TYPED_CLASS_CONST3 = 1;

    #[\Deprecated]
    public const int ZEND_TEST_DEPRECATED = 42;

    #[\Deprecated(message: "custom message")]
    public const int ZEND_TEST_DEPRECATED_ATTR = 42;

    public static $_StaticProp;
    public static int $staticIntProp = 123;

    public int $intProp = 123;
    public ?stdClass $classProp = null;
    public stdClass|Iterator|null $classUnionProp = null;
    public Traversable&Countable $classIntersectionProp;
    public readonly int $readonlyProp;
    public final int $finalProp;
    public Iterator|(Traversable&Countable) $dnfProperty;

    public static function is_object(): int { return 42; }

    #[\Deprecated]
    public function __toString(): string { return ""; }

    public function returnsStatic(): static { return new static; }

    public function returnsThrowable(): Throwable { throw new Error("Dummy"); }

    static public function variadicTest(string|Iterator ...$elements) : static { return new static; }

    public function takesUnionType(stdclass|Iterator $arg): void {}
}

class _ZendTestMagicCall
{
    public function __call(string $name, array $args): mixed { return [$name, $args]; }
}

class _ZendTestMagicCallForward
{
    public function __call(string $name, array $args): mixed { return $name(); }
}

class _ZendTestChildClass extends _ZendTestClass
{
    public function returnsThrowable(): Exception { throw new Error("Dummy"); }
}

final class ZendTestGenStubFlagCompatibilityTest {
}

class ZendAttributeTest {
    #[ZendTestRepeatableAttribute]
    #[ZendTestRepeatableAttribute]
    public const TEST_CONST = 1;

    #[ZendTestRepeatableAttribute]
    #[ZendTestPropertyAttribute("testProp")]
    public $testProp;

    #[ZendTestAttribute]
    public function testMethod(): bool { return true; }
}

trait _ZendTestTrait {
    public $testProp;
    public Traversable|Countable $classUnionProp;

    public function testMethod(): bool { return true; }
}

#[Attribute(Attribute::TARGET_ALL)]
final class ZendTestAttribute {
}

#[Attribute(Attribute::TARGET_ALL)]
final class ZendTestAttributeWithArguments {
    public readonly mixed $arg;

    public function __construct(mixed $arg) { $this->arg = $arg; }
}

#[Attribute(Attribute::TARGET_ALL|Attribute::IS_REPEATABLE)]
final class ZendTestRepeatableAttribute {
}

#[Attribute(Attribute::TARGET_PARAMETER)]
final class ZendTestParameterAttribute {
    public string $parameter;

    public function __construct(string $parameter) { $this->parameter = $parameter; }
}

/**
     * "Lorem ipsum"
     * @see https://www.php.net
     * @since 8.1
     */
#[Attribute(Attribute::TARGET_PROPERTY)]
final class ZendTestPropertyAttribute {
    /**
         * "Lorem ipsum"
         * @see https://www.php.net
         * @since 8.4
         */
    public string $parameter;

    public function __construct(string $parameter) { $this->parameter = $parameter; }
}

class ZendTestClassWithMethodWithParameterAttribute {
    final public function no_override(
        #[ZendTestParameterAttribute("value2")]
        string $parameter
    ): int { return 2; }
    public function override(
        #[ZendTestParameterAttribute("value3")]
        string $parameter
    ): int { return 3; }
}

class ZendTestChildClassWithMethodWithParameterAttribute extends ZendTestClassWithMethodWithParameterAttribute {
    public function override(
        #[ZendTestParameterAttribute("value4")]
        string $parameter
    ): int { return 4; }
}

class ZendTestClassWithPropertyAttribute {
    #[ZendTestAttribute]
    public string $attributed;
}

final class ZendTestForbidDynamicCall {
    public function call(): void {}
    public static function callStatic(): void {}
}

enum ZendTestUnitEnum {
    case Foo;
    case Bar;
}

enum ZendTestStringEnum: string {
    case Foo = "Test1";
    case Bar = 'Test2';
    case Baz = "Test2\\a";
    case FortyTwo = "42";
}

enum ZendTestIntEnum: int {
    case Foo = 1;
    case Bar = 3;
    case Baz = -1;
}

enum ZendTestEnumWithInterface implements _ZendTestInterface {
    case Foo;
    case Bar;
}

final class DoOperationNoCast {
    private int $val;
    public function __construct(int $val) { $this->val = $val; }
}

final class LongCastableNoOperations {
    private int $val;
    public function __construct(int $val) { $this->val = $val; }
}

final class FloatCastableNoOperations {
    private float $val;
    public function __construct(float $val) { $this->val = $val; }
}

final class NumericCastableNoOperations {
    private int|float $val;
    public function __construct(int|float $val) { $this->val = $val; }
}

class DimensionHandlersNoArrayAccess {
    public bool $read = false;
    public bool $write = false;
    public bool $has = false;
    public bool $unset = false;
    public int $readType;
    public bool $hasOffset = false;
    public int $checkEmpty;
    public mixed $offset;
}

}

namespace ZendTestNS {

class Foo {
    public function method(): int { return 0; }
}

class UnlikelyCompileError {
    public function method(): ?UnlikelyCompileError { return null; }
}

class NotUnlikelyCompileError {
    public function method(): ?NotUnlikelyCompileError { return null; }
}

}

namespace ZendTestNS2 {

class Foo {
    public ZendSubNS\Foo $foo;

    public function method(): void {}
}

}

namespace ZendTestNS2\ZendSubNS {

class Foo {
    public function method(): void {}
}

}

namespace ZendTest\Iterators {

final class TraversableTest implements \IteratorAggregate
{
    public function __construct() {}
    public function getIterator(): \Iterator {}
}

}

// Declarations of the extension's functions carrying what their stubs
// declare besides arginfo (attributes, exposed doc comments). The functions
// themselves are native; these are only kept for Reflection (see lib.rs).
namespace __zend_test_function_decls {

/**
     * "Lorem ipsum"
     * @see https://www.php.net
     * @since 8.3
     */
function zend_test_nullable_array_return(): null|array {}

#[\Deprecated(message: "custom message")]
function zend_test_deprecated_attr(): void {}

#[\NoDiscard(message: "custom message")]
function zend_test_nodiscard(): int {}

#[\Deprecated(message: "custom message")]
#[\NoDiscard(message: "custom message 2")]
function zend_test_deprecated_nodiscard(): int {}

function zend_test_parameter_with_attribute(
    #[\ZendTestParameterAttribute("value1")]
    string $parameter
): int {}

#[\ZendTestAttributeWithArguments(arg: "foo")]
function zend_test_attribute_with_named_argument(): void {}

// Registered from C code (register_dynamic_function_entries(), the legacy
// function table): no stub.
function zend_test_internal_dnf_arguments(
    \Iterator|(\Traversable&\Countable) $arg
): \Iterator|(\Traversable&\Countable) {}

function zend_iterable_legacy(iterable $arg1, ?iterable $arg2 = null): iterable {}

}
