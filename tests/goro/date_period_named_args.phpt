--TEST--
DatePeriod::__construct() named arguments
--FILE--
<?php
$dt = new DateTime('2024-01-01'); $iv = new DateInterval('P1D');
foreach ([
  fn() => new DatePeriod(start: $dt, interval: $iv, recurrences: 2),
  fn() => new DatePeriod(start: $dt, interval: $iv, end: 2),
  fn() => new DatePeriod($dt, $iv, end: new DateTime('2024-01-03')),
  fn() => new DatePeriod($dt, $iv, 2, options: DatePeriod::EXCLUDE_START_DATE),
  fn() => new DatePeriod('R2/2012-07-01T00:00:00Z/P7D', options: 1),
  fn() => new DatePeriod($dt, interval: $iv, end: 3, options: 0),
] as $f) {
  try { $p = $f(); foreach ($p as $d) echo $d->format('Y-m-d '), ''; echo "\n"; } catch (Throwable $e) { echo get_class($e), ": ", $e->getMessage(), "\n"; }
}
--EXPECT--
Error: Unknown named parameter $recurrences
2024-01-01 2024-01-02 2024-01-03 
2024-01-01 2024-01-02 
2024-01-02 2024-01-03 
ArgumentCountError: DatePeriod::__construct(): Argument #2 ($interval) must be passed explicitly, because the default value is not known
2024-01-01 2024-01-02 2024-01-03 2024-01-04 
