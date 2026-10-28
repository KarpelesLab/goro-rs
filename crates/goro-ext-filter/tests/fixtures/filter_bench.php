<?php
// PHP side of examples/filter_bench.rs: ns per filter_var() call, minus the
// cost of the same loop without the call.
//
//   php -n filter_bench.php

const N = 1000000;

function bench(string $name, string $v, int $filter, $options = 0): void {
    for ($i = 0; $i < 10000; $i++) {
        filter_var($v, $filter, $options);
    }
    $t = hrtime(true);
    for ($i = 0; $i < N; $i++) {
        $r = filter_var($v, $filter, $options);
    }
    $call = hrtime(true) - $t;
    $t = hrtime(true);
    for ($i = 0; $i < N; $i++) {
        $r = $v;
    }
    $loop = hrtime(true) - $t;
    printf("%-28s %7.1f ns/call (%.1f with the loop)\n", $name, ($call - $loop) / N, $call / N);
}

$range = ['options' => ['min_range' => 1, 'max_range' => 100000]];
bench('int', '12345', FILTER_VALIDATE_INT);
bench('int min/max', '12345', FILTER_VALIDATE_INT, $range);
bench('float', '12345.678', FILTER_VALIDATE_FLOAT);
bench('bool', 'yes', FILTER_VALIDATE_BOOL);
bench('email', 'user.name@example.com', FILTER_VALIDATE_EMAIL);
bench('ipv4', '192.168.1.1', FILTER_VALIDATE_IP);
bench('ipv6', '2001:db8::ff00:42:8329', FILTER_VALIDATE_IP);
bench('url', 'http://www.example.com/path?q=1', FILTER_VALIDATE_URL);
bench('domain hostname', 'www.example.com', FILTER_VALIDATE_DOMAIN, FILTER_FLAG_HOSTNAME);
bench('special_chars', '<b>hello</b> & world', FILTER_SANITIZE_SPECIAL_CHARS);
