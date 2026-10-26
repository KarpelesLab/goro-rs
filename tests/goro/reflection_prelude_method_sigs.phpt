--TEST--
Methods internal classes declare from PHP source reflect their stub signatures (BcMath\Number)
--FILE--
<?php
$r = new ReflectionClass('BcMath\Number');
foreach ($r->getMethods() as $m) {
    echo $m->getName(), "(";
    $ps = [];
    foreach ($m->getParameters() as $p) {
        $ps[] = ($p->getType() ? $p->getType() . ' ' : '') . ($p->isVariadic() ? '...' : '') . '$' . $p->getName() . ($p->isOptional() && !$p->isVariadic() ? ' = ' . var_export($p->getDefaultValue(), true) : '');
    }
    echo implode(', ', $ps), ")", $m->hasReturnType() ? ': ' . $m->getReturnType() : '', "\n";
}
echo new ReflectionMethod('BcMath\Number', 'add');
--EXPECT--
__construct(string|int $num)
add(BcMath\Number|string|int $num, ?int $scale = NULL): BcMath\Number
sub(BcMath\Number|string|int $num, ?int $scale = NULL): BcMath\Number
mul(BcMath\Number|string|int $num, ?int $scale = NULL): BcMath\Number
div(BcMath\Number|string|int $num, ?int $scale = NULL): BcMath\Number
mod(BcMath\Number|string|int $num, ?int $scale = NULL): BcMath\Number
divmod(BcMath\Number|string|int $num, ?int $scale = NULL): array
powmod(BcMath\Number|string|int $exponent, BcMath\Number|string|int $modulus, ?int $scale = NULL): BcMath\Number
pow(BcMath\Number|string|int $exponent, ?int $scale = NULL): BcMath\Number
sqrt(?int $scale = NULL): BcMath\Number
floor(): BcMath\Number
ceil(): BcMath\Number
round(int $precision = 0, RoundingMode $mode = \RoundingMode::HalfAwayFromZero): BcMath\Number
compare(BcMath\Number|string|int $num, ?int $scale = NULL): int
__toString(): string
__serialize(): array
__unserialize(array $data): void
Method [ <internal:bcmath> public method add ] {

  - Parameters [2] {
    Parameter #0 [ <required> BcMath\Number|string|int $num ]
    Parameter #1 [ <optional> ?int $scale = null ]
  }
  - Return [ BcMath\Number ]
}
