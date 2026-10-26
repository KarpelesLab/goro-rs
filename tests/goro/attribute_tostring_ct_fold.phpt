--TEST--
ReflectionAttribute::__toString() shows the arguments as the compiler folded them (internal constants, class constants bound at compile time)
--FILE--
<?php
namespace N {
    #[\Attribute]
    class A { function __construct(...$a) {} }
    class K { const B = 2; protected const P = 1; }
    enum E { case X; const Y = 5; }
    #[A(PHP_EOL, \PHP_EOL, K::B, \N\K::B, E::X, E::Y, [1 + 1, FOO, \E_ALL], \FILE_TEXT, \ArrayObject::ARRAY_AS_PROPS, \DATE_ATOM)]
    function f() {}
    echo (new \ReflectionFunction('N\f'))->getAttributes()[0];
    class M {
        const Q = 7;
        #[A(self::Q, M::Q, self::class)]
        function m() {}
    }
    echo (new \ReflectionMethod('N\M', 'm'))->getAttributes()[0];
    class P extends K { const Z = 3; }
    #[A(P::Z, P::B, K::P, M_PI * 2, -PHP_INT_MAX, PHP_INT_SIZE > 4 ? 'y' : 'n', true && false)]
    function g() {}
    echo (new \ReflectionFunction('N\g'))->getAttributes()[0];
}
--EXPECT--
Attribute [ N\A ] {
  - Arguments [10] {
    Argument #0 [ N\PHP_EOL ]
    Argument #1 [ '\n' ]
    Argument #2 [ 2 ]
    Argument #3 [ 2 ]
    Argument #4 [ \N\E::X ]
    Argument #5 [ \N\E::Y ]
    Argument #6 [ [2, N\FOO, 30719] ]
    Argument #7 [ FILE_TEXT ]
    Argument #8 [ 2 ]
    Argument #9 [ 'Y-m-d\\TH:i:sP' ]
  }
}
Attribute [ N\A ] {
  - Arguments [3] {
    Argument #0 [ 7 ]
    Argument #1 [ 7 ]
    Argument #2 [ 'N\\M' ]
  }
}
Attribute [ N\A ] {
  - Arguments [7] {
    Argument #0 [ 3 ]
    Argument #1 [ 2 ]
    Argument #2 [ \N\K::P ]
    Argument #3 [ N\M_PI * 2 ]
    Argument #4 [ -N\PHP_INT_MAX ]
    Argument #5 [ N\PHP_INT_SIZE > 4 ? 'y' : 'n' ]
    Argument #6 [ false ]
  }
}
