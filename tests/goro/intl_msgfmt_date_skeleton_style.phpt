--TEST--
MessageFormatter: date/time skeleton styles (::Hmm) and date patterns written with spaces
--EXTENSIONS--
intl
--INI--
date.timezone=UTC
--FILE--
<?php
// a skeleton gets the pattern generator's best pattern for the locale
// (DateFormat::createInstanceForSkeleton()); a date pattern style is used
// as written, its surrounding white space included, like number patterns
foreach (["{0, time, ::Hmm}", "{0,time,::jmm}", "{0, date, ::yMMMd}", "{0, date, ::MMMMd}", "[{0, date, yyyy-MM-dd }]", "[{0,date,yyyy}]", "[{0, number, #,##0.0}]", "[{0, date, short }]"] as $p) {
    foreach (["en_US", "de_DE"] as $loc) {
        echo $p, " ", $loc, ": ", MessageFormatter::formatMessage($loc, $p, [1700000000]), "\n";
    }
}
?>
--EXPECT--
{0, time, ::Hmm} en_US: 22:13
{0, time, ::Hmm} de_DE: 22:13
{0,time,::jmm} en_US: 10:13 PM
{0,time,::jmm} de_DE: 22:13
{0, date, ::yMMMd} en_US: Nov 14, 2023
{0, date, ::yMMMd} de_DE: 14. Nov. 2023
{0, date, ::MMMMd} en_US: November 14
{0, date, ::MMMMd} de_DE: 14. November
[{0, date, yyyy-MM-dd }] en_US: [ 2023-11-14 ]
[{0, date, yyyy-MM-dd }] de_DE: [ 2023-11-14 ]
[{0,date,yyyy}] en_US: [2023]
[{0,date,yyyy}] de_DE: [2023]
[{0, number, #,##0.0}] en_US: [ 1,700,000,000.0]
[{0, number, #,##0.0}] de_DE: [ 1.700.000.000,0]
[{0, date, short }] en_US: [11/14/23]
[{0, date, short }] de_DE: [14.11.23]
