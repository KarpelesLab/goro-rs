--TEST--
iconv(): ISO-2022-KR, and unknown ISO-2022 escapes as characters
--FILE--
<?php
function h($x) { return $x === false ? "F" : bin2hex($x); }
foreach (["", "a", "한", "a한b", "한\n한", "€", "\x0e", "日本", "\u{20A9}"] as $s) {
    echo json_encode($s), " ", h(@iconv("UTF-8", "ISO-2022-KR", $s)), "\n";
}
foreach (["ISO-2022-KR" => ["\x1b\$)Ca\x0e\x47\x51\x0fb", "a\x0e\x47\x51\x0fb", "\x1b\$)C\x0e\x47", "\x1b\$)C\x0e\x47\x51\n\x47\x51", "\x1b\$)C\x0e\x7f\x0f", "\x1b\$)", "\x80", "\x0e\x0a", "\x1b\x66", "\x0e\x1b\x39\x29", "\x1b\$)C\x0e\x47\xd1\x0f"],
          "ISO-2022-JP" => ["\x1b)\x0e\x0e", "\x1b\x61\x82", "\x1b\$(", "\x1b\$B\x46\x7c\x1b(B", "\x1b(", "\x1bx"]] as $cs => $list) {
    foreach ($list as $s) {
        foreach (["", "//IGNORE"] as $m) {
            error_clear_last();
            $r = @iconv($cs, "UTF-8$m", $s);
            echo "$cs$m ", bin2hex($s), " ", h($r), " ", error_get_last()['message'] ?? '', "\n";
        }
    }
}
var_dump(iconv_strlen("\x1b\$)Ca\x0e\x47\x51\x0fb", "ISO-2022-KR"));
var_dump(iconv_mime_encode("S", "한국어", ["output-charset" => "ISO-2022-KR", "scheme" => "B"]));
--EXPECT--
"" 1b242943
"a" 1b24294361
"\ud55c" 1b2429430e47510f
"a\ud55cb" 1b242943610e47510f62
"\ud55c\n\ud55c" 1b2429430e47510f0a0e47510f
"\u20ac" 1b2429430e22660f
"\u000e" 1b2429430e
"\u65e5\u672c" 1b2429430e6c6d5c620f
"\u20a9" F
ISO-2022-KR 1b242943610e47510f62 61ed959c62 
ISO-2022-KR//IGNORE 1b242943610e47510f62 61ed959c62 
ISO-2022-KR 610e47510f62 61ed959c62 
ISO-2022-KR//IGNORE 610e47510f62 61ed959c62 
ISO-2022-KR 1b2429430e47 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR//IGNORE 1b2429430e47 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR 1b2429430e47510a4751 F iconv(): Detected an illegal character in input string
ISO-2022-KR//IGNORE 1b2429430e47510a4751 ed959ced959c 
ISO-2022-KR 1b2429430e7f0f F iconv(): Detected an illegal character in input string
ISO-2022-KR//IGNORE 1b2429430e7f0f  
ISO-2022-KR 1b2429 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR//IGNORE 1b2429 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR 80 F iconv(): Detected an illegal character in input string
ISO-2022-KR//IGNORE 80  
ISO-2022-KR 0e0a F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR//IGNORE 0e0a F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-KR 1b66 1b66 
ISO-2022-KR//IGNORE 1b66 1b66 
ISO-2022-KR 0e1b3929 F iconv(): Detected an illegal character in input string
ISO-2022-KR//IGNORE 0e1b3929 ebaca9 
ISO-2022-KR 1b2429430e47d10f F iconv(): Detected an illegal character in input string
ISO-2022-KR//IGNORE 1b2429430e47d10f  
ISO-2022-JP 1b290e0e 1b290e0e 
ISO-2022-JP//IGNORE 1b290e0e 1b290e0e 
ISO-2022-JP 1b6182 F iconv(): Detected an illegal character in input string
ISO-2022-JP//IGNORE 1b6182 1b61 
ISO-2022-JP 1b2428 1b2428 
ISO-2022-JP//IGNORE 1b2428 1b2428 
ISO-2022-JP 1b2442467c1b2842 e697a5 
ISO-2022-JP//IGNORE 1b2442467c1b2842 e697a5 
ISO-2022-JP 1b28 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-JP//IGNORE 1b28 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-JP 1b78 F iconv(): Detected an incomplete multibyte character in input string
ISO-2022-JP//IGNORE 1b78 F iconv(): Detected an incomplete multibyte character in input string
int(3)
string(37) "S: =?ISO-2022-KR?B?GyQpQw5HUTE5Pm4P?="
