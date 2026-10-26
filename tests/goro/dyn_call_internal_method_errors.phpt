--TEST--
Dynamic calls of internal classes' methods: undefined / non-static / abstract
--FILE--
<?php
foreach ([[[new DateTime('2020-01-01'), 'format'], ['Y']], [['DateTime', 'createFromFormat'], ['Y', '2020']], [['DateTime', 'format'], ['Y']], [[new ArrayObject([1, 2]), 'count'], []],
  [['ArrayObject', 'count'], []], ['DateTime::createFromFormat', ['Y', '2021']], [['Closure', 'fromCallable'], ['strlen']], [[new ArrayIterator([1]), 'COUNT'], []],
  [[new SplObjectStorage, 'nope'], []], [['Random\Randomizer', 'nope'], []], [['WeakMap', 'nope'], []], [['Generator', 'nope'], []], [['UnitEnum', 'cases'], []]] as [$f, $a]) {
  try { $r = $f(...$a); echo is_object($r) ? get_class($r) : json_encode($r), "\n"; } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
}
class U {}
foreach ([[new U, 'nope'], [new ArrayObject, 'nope'], [new DateTime, 'nope'], ['DateTime', 'nope'], ['U', 'nope'], [new DateTimeZone('UTC'), 'nope'], 'DateTime::nope', [new SplStack, 'nope'], [new ReflectionClass('U'), 'nope']] as $f) {
  try { $f(); } catch (Throwable $e) { echo get_class($e), ': ', $e->getMessage(), "\n"; }
}
--EXPECT--
"2020"
DateTime
Error: Non-static method DateTime::format() cannot be called statically
2
Error: Non-static method ArrayObject::count() cannot be called statically
DateTime
Closure
1
Error: Call to undefined method SplObjectStorage::nope()
Error: Call to undefined method Random\Randomizer::nope()
Error: Call to undefined method WeakMap::nope()
Error: Call to undefined method Generator::nope()
Error: Cannot call abstract method UnitEnum::cases()
Error: Call to undefined method U::nope()
Error: Call to undefined method ArrayObject::nope()
Error: Call to undefined method DateTime::nope()
Error: Call to undefined method DateTime::nope()
Error: Call to undefined method U::nope()
Error: Call to undefined method DateTimeZone::nope()
Error: Call to undefined method DateTime::nope()
Error: Call to undefined method SplStack::nope()
Error: Call to undefined method ReflectionClass::nope()
