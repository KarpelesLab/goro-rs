--TEST--
IntlDateFormatter::parse() with setLenient(false) fails on fields the calendar refuses
--EXTENSIONS--
intl
--FILE--
<?php
// a non-lenient calendar validates the parsed fields: month 13, February
// 30th, minute 61 fail the parse (U_PARSE_ERROR, the position after the
// fields) instead of giving 0; a lenient one rolls them over
$p = new IntlDateFormatter("en_US", IntlDateFormatter::NONE, IntlDateFormatter::NONE, "UTC", null, "yyyy-MM-dd HH:mm");
foreach ([true, false] as $lenient) {
    $p->setLenient($lenient);
    foreach (["2023-11-14 22:13", "2023-13-45 22:13", "2023-02-30 10:00", "2024-02-29 10:00", "2023-02-29 10:00", "2023-11-14 22:61"] as $s) {
        $pos = 0;
        $r = $p->parse($s, $pos);
        echo $lenient ? "lenient " : "strict  ", $s, " => ", var_export($r, true), " pos=$pos ", $p->getErrorMessage(), "\n";
    }
}
var_dump(datefmt_localtime($p, "2023-13-01 00:00"), intl_get_error_code());
?>
--EXPECT--
lenient 2023-11-14 22:13 => 1699999980 pos=16 U_ZERO_ERROR
lenient 2023-13-45 22:13 => 1707948780 pos=16 U_ZERO_ERROR
lenient 2023-02-30 10:00 => 1677751200 pos=16 U_ZERO_ERROR
lenient 2024-02-29 10:00 => 1709200800 pos=16 U_ZERO_ERROR
lenient 2023-02-29 10:00 => 1677664800 pos=16 U_ZERO_ERROR
lenient 2023-11-14 22:61 => 1700002860 pos=16 U_ZERO_ERROR
strict  2023-11-14 22:13 => 1699999980 pos=16 U_ZERO_ERROR
strict  2023-13-45 22:13 => false pos=16 IntlDateFormatter::parse(): Date parsing failed: U_PARSE_ERROR
strict  2023-02-30 10:00 => false pos=16 IntlDateFormatter::parse(): Date parsing failed: U_PARSE_ERROR
strict  2024-02-29 10:00 => 1709200800 pos=16 U_ZERO_ERROR
strict  2023-02-29 10:00 => false pos=16 IntlDateFormatter::parse(): Date parsing failed: U_PARSE_ERROR
strict  2023-11-14 22:61 => false pos=16 IntlDateFormatter::parse(): Date parsing failed: U_PARSE_ERROR
bool(false)
int(9)
