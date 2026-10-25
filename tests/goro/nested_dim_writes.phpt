--TEST--
Nested array writes through write fetches
--FILE--
<?php
function run() {
    $a = [];
    for ($i = 0; $i < 4; $i++) { $a[$i % 2]['k'][$i] = $i; $a['x'][][] = $i; }
    $b = $a; $a[0]['k']['new'] = 1; $a['x'][0][] = 'z';
    var_dump($a, $b);
    $c = ['n' => null, 'f' => false, 's' => 'str', 'i' => 5];
    $c['n']['a'] = 1;
    $c['f']['a'] = 1;
    try { $c['i']['a'] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
    $c['s'][0] = 'X';
    var_dump($c);
    $d = [[1, 2]]; $e = &$d[0]; $d[0][] = 3; var_dump($e);
    $f = [[1]]; $g = $f[0]; $f[0][5] = 6; var_dump($g, $f);
    $h = ['a' => ['b' => ['c' => 1]]]; $h2 = $h; $h['a']['b']['d'] = 2; var_dump($h, $h2);
    $big = []; $big[PHP_INT_MAX] = 1; try { $big[][] = 1; } catch (Error $e) { echo $e->getMessage(), "\n"; }
    $l = [1 => [], "1" => [], "01" => []]; $l["1"][] = 'x'; $l["01"][] = 'y'; var_dump($l);
}
run();
$ga = [];
for ($i = 0; $i < 3; $i++) { $ga['q'][$i][] = $i; }
var_dump($ga);
--EXPECTF--
array(3) {
  [0]=>
  array(1) {
    ["k"]=>
    array(3) {
      [0]=>
      int(0)
      [2]=>
      int(2)
      ["new"]=>
      int(1)
    }
  }
  ["x"]=>
  array(4) {
    [0]=>
    array(2) {
      [0]=>
      int(0)
      [1]=>
      string(1) "z"
    }
    [1]=>
    array(1) {
      [0]=>
      int(1)
    }
    [2]=>
    array(1) {
      [0]=>
      int(2)
    }
    [3]=>
    array(1) {
      [0]=>
      int(3)
    }
  }
  [1]=>
  array(1) {
    ["k"]=>
    array(2) {
      [1]=>
      int(1)
      [3]=>
      int(3)
    }
  }
}
array(3) {
  [0]=>
  array(1) {
    ["k"]=>
    array(2) {
      [0]=>
      int(0)
      [2]=>
      int(2)
    }
  }
  ["x"]=>
  array(4) {
    [0]=>
    array(1) {
      [0]=>
      int(0)
    }
    [1]=>
    array(1) {
      [0]=>
      int(1)
    }
    [2]=>
    array(1) {
      [0]=>
      int(2)
    }
    [3]=>
    array(1) {
      [0]=>
      int(3)
    }
  }
  [1]=>
  array(1) {
    ["k"]=>
    array(2) {
      [1]=>
      int(1)
      [3]=>
      int(3)
    }
  }
}

Deprecated: Automatic conversion of false to array is deprecated in %s on line 9
Cannot use a scalar value as an array
array(4) {
  ["n"]=>
  array(1) {
    ["a"]=>
    int(1)
  }
  ["f"]=>
  array(1) {
    ["a"]=>
    int(1)
  }
  ["s"]=>
  string(3) "Xtr"
  ["i"]=>
  int(5)
}
array(3) {
  [0]=>
  int(1)
  [1]=>
  int(2)
  [2]=>
  int(3)
}
array(1) {
  [0]=>
  int(1)
}
array(1) {
  [0]=>
  array(2) {
    [0]=>
    int(1)
    [5]=>
    int(6)
  }
}
array(1) {
  ["a"]=>
  array(1) {
    ["b"]=>
    array(2) {
      ["c"]=>
      int(1)
      ["d"]=>
      int(2)
    }
  }
}
array(1) {
  ["a"]=>
  array(1) {
    ["b"]=>
    array(1) {
      ["c"]=>
      int(1)
    }
  }
}
Cannot add element to the array as the next element is already occupied
array(2) {
  [1]=>
  array(1) {
    [0]=>
    string(1) "x"
  }
  ["01"]=>
  array(1) {
    [0]=>
    string(1) "y"
  }
}
array(1) {
  ["q"]=>
  array(3) {
    [0]=>
    array(1) {
      [0]=>
      int(0)
    }
    [1]=>
    array(1) {
      [0]=>
      int(1)
    }
    [2]=>
    array(1) {
      [0]=>
      int(2)
    }
  }
}
