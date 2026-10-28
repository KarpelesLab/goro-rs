<?php
// Driver for mt_rand.txt (one php85 run): sequences after mt_srand(seed[, mode]).
// Each line: seed mode range-label : values.
error_reporting(E_ALL & ~E_DEPRECATED);
$ranges = [
  'raw' => null,
  '0-9' => [0, 9], '1-6' => [1, 6], '0-999' => [0, 999], 'neg' => [-1000, 1000], '0-1' => [0, 1],
  'pow2' => [0, 1023], 'u32' => [0, 4294967295], 'u32m' => [0, 4294967294], 'u32p' => [0, 4294967296],
  'big' => [0, PHP_INT_MAX], 'full' => [PHP_INT_MIN, PHP_INT_MAX], 'neg64' => [-5000000000, 5000000000],
  'x666' => [0, 0x66666665], 'same' => [5, 5], 'i31' => [0, 2147483647], 'i31p' => [0, 2147483648],
];
foreach ([0, 1, 12345678, 1234567890, -1, PHP_INT_MAX, 4294967296 + 7, -2147483648] as $seed) {
  foreach ([MT_RAND_MT19937, MT_RAND_PHP] as $mode) {
    foreach ($ranges as $label => $r) {
      mt_srand($seed, $mode);
      $vals = [];
      for ($i = 0; $i < 12; $i++) $vals[] = $r === null ? mt_rand() : mt_rand($r[0], $r[1]);
      echo "$seed $mode $label: ", implode(' ', $vals), "\n";
    }
    mt_srand($seed, $mode);
    $vals = [];
    for ($i = 0; $i < 12; $i++) $vals[] = rand(20, 10);
    echo "$seed $mode rand20-10: ", implode(' ', $vals), "\n";
    mt_srand($seed, $mode);
    $vals = [];
    for ($i = 0; $i < 12; $i++) $vals[] = rand();
    echo "$seed $mode randraw: ", implode(' ', $vals), "\n";
    mt_srand($seed, $mode);
    $vals = [];
    for ($i = 0; $i < 12; $i++) $vals[] = rand(0, 99);
    echo "$seed $mode rand0-99: ", implode(' ', $vals), "\n";
    mt_srand($seed, $mode);
    $x = 0;
    for ($i = 0; $i < 2000; $i++) $x ^= mt_rand();
    echo "$seed $mode xor2000: $x\n";
  }
}
try { mt_rand(10, 1); } catch (ValueError $e) { echo "err: ", $e->getMessage(), "\n"; }
try { mt_rand(1); } catch (Throwable $e) { echo "err: ", get_class($e), ": ", $e->getMessage(), "\n"; }
