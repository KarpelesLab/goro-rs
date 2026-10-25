--TEST--
GMP number strings: no white space between the sign and the digits
--FILE--
<?php
foreach (["- 1", "--1", "+-1", "-+1", "+ 1", " -1", "- 0x1", "-0x1", "-0b1", "-01", "- 01", "+1", "+0x1", "1 ", "1\t", " 1 ", "0x 1", "0 x1"] as $s) {
  try { echo json_encode($s), " => ", gmp_init($s), "\n"; } catch (Throwable $e) { echo json_encode($s), " => ", $e->getMessage(), "\n"; }
  try { echo json_encode($s), " +=> ", gmp_init(1) + $s, "\n"; } catch (Throwable $e) { echo json_encode($s), " +=> ", $e->getMessage(), "\n"; }
}
foreach (["- 1", " 1", "+ 1", "1 "] as $s) {
  try { echo json_encode($s), " 16=> ", gmp_init($s, 16), "\n"; } catch (Throwable $e) { echo $e->getMessage(), "\n"; }
}
--EXPECT--
"- 1" => "- 1" => gmp_init(): Argument #1 ($num) is not an integer string
"- 1" +=> "- 1" +=> Number is not an integer string
"--1" => "--1" => gmp_init(): Argument #1 ($num) is not an integer string
"--1" +=> "--1" +=> Number is not an integer string
"+-1" => "+-1" => gmp_init(): Argument #1 ($num) is not an integer string
"+-1" +=> "+-1" +=> Number is not an integer string
"-+1" => "-+1" => gmp_init(): Argument #1 ($num) is not an integer string
"-+1" +=> "-+1" +=> Number is not an integer string
"+ 1" => "+ 1" => gmp_init(): Argument #1 ($num) is not an integer string
"+ 1" +=> "+ 1" +=> Number is not an integer string
" -1" => -1
" -1" +=> 0
"- 0x1" => "- 0x1" => gmp_init(): Argument #1 ($num) is not an integer string
"- 0x1" +=> "- 0x1" +=> Number is not an integer string
"-0x1" => -1
"-0x1" +=> 0
"-0b1" => -1
"-0b1" +=> 0
"-01" => -1
"-01" +=> 0
"- 01" => "- 01" => gmp_init(): Argument #1 ($num) is not an integer string
"- 01" +=> "- 01" +=> Number is not an integer string
"+1" => "+1" => gmp_init(): Argument #1 ($num) is not an integer string
"+1" +=> "+1" +=> Number is not an integer string
"+0x1" => "+0x1" => gmp_init(): Argument #1 ($num) is not an integer string
"+0x1" +=> "+0x1" +=> Number is not an integer string
"1 " => 1
"1 " +=> 2
"1\t" => 1
"1\t" +=> 2
" 1 " => 1
" 1 " +=> 2
"0x 1" => 1
"0x 1" +=> 2
"0 x1" => "0 x1" => gmp_init(): Argument #1 ($num) is not an integer string
"0 x1" +=> "0 x1" +=> Number is not an integer string
"- 1" 16=> gmp_init(): Argument #1 ($num) is not an integer string
" 1" 16=> 1
"+ 1" 16=> gmp_init(): Argument #1 ($num) is not an integer string
"1 " 16=> 1
