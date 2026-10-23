--TEST--
createFromFormat signed-number errors are reported at position 0
--FILE--
<?php
DateTime::createFromFormat('!YU', '20241234567890123456789012345'); print_r(DateTime::getLastErrors());
print_r(date_parse_from_format('U', 'x123'));
print_r(date_parse_from_format('Y-m-d X', '2024-01-01 99999999999999999999999 junk'));
print_r(date_parse_from_format('Y-m-d x', '2024-01-01 +'));
print_r(date_parse_from_format('d U', '12 -'));
--EXPECT--
Array
(
    [warning_count] => 0
    [warnings] => Array
        (
        )

    [error_count] => 2
    [errors] => Array
        (
            [0] => Number out of range
            [28] => Trailing data
        )

)
Array
(
    [year] => 1970
    [month] => 1
    [day] => 1
    [hour] => 0
    [minute] => 2
    [second] => 3
    [fraction] => 0
    [warning_count] => 0
    [warnings] => Array
        (
        )

    [error_count] => 1
    [errors] => Array
        (
            [0] => Unexpected data found.
        )

    [is_localtime] => 1
    [zone_type] => 1
    [zone] => 0
    [is_dst] => 
)
Array
(
    [year] => 0
    [month] => 1
    [day] => 1
    [hour] => 
    [minute] => 
    [second] => 
    [fraction] => 
    [warning_count] => 0
    [warnings] => Array
        (
        )

    [error_count] => 2
    [errors] => Array
        (
            [0] => Number out of range
            [30] => Trailing data
        )

    [is_localtime] => 
)
Array
(
    [year] => 0
    [month] => 1
    [day] => 1
    [hour] => 
    [minute] => 
    [second] => 
    [fraction] => 
    [warning_count] => 0
    [warnings] => Array
        (
        )

    [error_count] => 1
    [errors] => Array
        (
            [0] => Found unexpected data
        )

    [is_localtime] => 
)
Array
(
    [year] => 1970
    [month] => 1
    [day] => 1
    [hour] => 0
    [minute] => 0
    [second] => 0
    [fraction] => 0
    [warning_count] => 0
    [warnings] => Array
        (
        )

    [error_count] => 1
    [errors] => Array
        (
            [0] => Found unexpected data
        )

    [is_localtime] => 1
    [zone_type] => 1
    [zone] => 0
    [is_dst] => 
)
