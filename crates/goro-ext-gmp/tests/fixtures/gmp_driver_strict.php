<?php
declare(strict_types=1);
// Calls made from this file use strict_types (used by gmp_driver.php).

function strict_call(string $fn, array $args) {
    return $fn(...$args);
}
