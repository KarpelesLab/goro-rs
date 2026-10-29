<?php
/* Classes of the zend_test emulation, written in PHP. Their shape (names,
 * constants, typed properties, attributes, doc comments, method results)
 * comes from the expected output of the php-src tests that use them:
 * ext/zend_test/tests/*.phpt, Zend/tests/{enum,type_declarations,
 * inheritance,traits,...}, ext/reflection/tests/internal_*.phpt. Methods
 * with an empty body listed in the module's `methods` run natively. */

interface _ZendTestInterface {
    /**
         * "Lorem ipsum"
         * @see https://www.php.net
         * @since 8.2
         */
    public const int DUMMY = 0;
}

class _ZendTestClass implements _ZendTestInterface {
    public const mixed TYPED_CLASS_CONST1 = [];
    public const int|string TYPED_CLASS_CONST2 = 42;
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
    final public int $finalProp;
    public Iterator|(Traversable&Countable) $dnfProperty;

    public static function is_object(): int { return 0; }

    #[\Deprecated]
    public function __toString(): string { return ""; }

    public function returnsStatic(): static { return new static(); }

    public function returnsThrowable(): Throwable { throw new Exception(); }

    public static function variadicTest(string|Iterator ...$elements): static { return new static(); }

    public function takesUnionType(stdClass|Iterator $arg): void {}

    public static function test(string $arg1 = "", string $arg2 = ""): string { return "test"; }
}

class _ZendTestChildClass extends _ZendTestClass {
    public function returnsThrowable(): Exception { throw new Exception(); }
}

trait _ZendTestTrait {
    public $testProp;
    public Traversable|Countable $classUnionProp;

    public function testMethod(): bool { return true; }
}

class _ZendTestMagicCall {
    public function __call(string $name, array $args): mixed { return [$name, $args]; }
}

class _ZendTestMagicCallForward {
    public function __call(string $name, array $args): mixed {}
}

#[Attribute(Attribute::TARGET_ALL)]
final class ZendTestAttribute {
}

#[Attribute(Attribute::TARGET_ALL)]
final class ZendTestAttributeWithArguments {
    public function __construct(public readonly mixed $arg) {}
}

#[Attribute(Attribute::TARGET_ALL | Attribute::IS_REPEATABLE)]
final class ZendTestRepeatableAttribute {
}

#[Attribute(Attribute::TARGET_PARAMETER)]
final class ZendTestParameterAttribute {
    public function __construct(public string $parameter) {}
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
    final public function no_override(#[ZendTestParameterAttribute("value2")] string $parameter): int { return 2; }
    public function override(#[ZendTestParameterAttribute("value3")] string $parameter): int { return 3; }
}

class ZendTestChildClassWithMethodWithParameterAttribute extends ZendTestClassWithMethodWithParameterAttribute {
    public function override(#[ZendTestParameterAttribute("value4")] string $parameter): int { return 4; }
}

final class ZendTestClassWithPropertyAttribute {
    #[ZendTestAttribute]
    public string $attributed;
}

class ZendAttributeTest {
    #[ZendTestRepeatableAttribute]
    #[ZendTestRepeatableAttribute]
    public const int TEST_CONST = 1;

    #[ZendTestRepeatableAttribute]
    #[ZendTestPropertyAttribute("testProp")]
    public mixed $testProp = null;

    #[ZendTestAttribute]
    public function testMethod(): bool { return true; }
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
    case Bar = "Test2";
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
