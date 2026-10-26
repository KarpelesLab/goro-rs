<?php
// BcMath\Number (bcmath.stub.php), declared from PHP source when the
// extension is registered and then made an internal class (see number.rs).
// The methods take their arguments as a variadic list and hand them over
// to the native implementation, which parses them like the C methods
// (zend_parse_parameters()). The value and scale properties are real
// readonly properties holding the canonical string and scale of the
// number.

namespace BcMath;

final readonly class Number implements \Stringable
{
    public string $value;
    public int $scale;

    public function __construct(...$args) { \__bcmath_number('__construct', $this, $args); }

    public function add(...$args) { return \__bcmath_number('add', $this, $args); }

    public function sub(...$args) { return \__bcmath_number('sub', $this, $args); }

    public function mul(...$args) { return \__bcmath_number('mul', $this, $args); }

    public function div(...$args) { return \__bcmath_number('div', $this, $args); }

    public function mod(...$args) { return \__bcmath_number('mod', $this, $args); }

    public function divmod(...$args) { return \__bcmath_number('divmod', $this, $args); }

    public function powmod(...$args) { return \__bcmath_number('powmod', $this, $args); }

    public function pow(...$args) { return \__bcmath_number('pow', $this, $args); }

    public function sqrt(...$args) { return \__bcmath_number('sqrt', $this, $args); }

    public function floor(...$args) { return \__bcmath_number('floor', $this, $args); }

    public function ceil(...$args) { return \__bcmath_number('ceil', $this, $args); }

    public function round(...$args) { return \__bcmath_number('round', $this, $args); }

    public function compare(...$args) { return \__bcmath_number('compare', $this, $args); }

    public function __toString(): string { return \func_num_args() ? \__bcmath_number('__toString', $this, \func_get_args()) : $this->value; }

    public function __serialize(): array { return \__bcmath_number('__serialize', $this, []); }

    public function __unserialize($data): void { \__bcmath_number('__unserialize', $this, [$data]); }
}
