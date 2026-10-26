--TEST--
Reflection __toString: interface parents, enum-typed constants, implicit Stringable
--FILE--
<?php
interface A {}
interface I1 {}
interface I2 extends I1 {}
interface B extends A {}
interface C {}
interface D extends B, C {}
echo new ReflectionClass('I2');
echo new ReflectionClass('D');
enum Suit { case H; const Wild = self::H; }
echo new ReflectionClass('Suit');
echo new ReflectionClassConstant('Suit', 'Wild');
echo new ReflectionClassConstant('Suit', 'H');
class S { function __toString(): string { return ""; } }
echo new ReflectionClass('S');
--EXPECTF--
Interface [ <user> interface I2 extends I1 ] {
  @@ %s 4-4

  - Constants [0] {
  }

  - Static properties [0] {
  }

  - Static methods [0] {
  }

  - Properties [0] {
  }

  - Methods [0] {
  }
}
Interface [ <user> interface D extends B, C, A ] {
  @@ %s 7-7

  - Constants [0] {
  }

  - Static properties [0] {
  }

  - Static methods [0] {
  }

  - Properties [0] {
  }

  - Methods [0] {
  }
}
Enum [ <user> enum Suit implements UnitEnum ] {
  @@ %s 10-10

  - Enum cases [1] {
    Case H
  }

  - Constants [1] {
    Constant [ public Suit Wild ] { Object }
  }

  - Static properties [0] {
  }

  - Static methods [1] {
    Method [ <internal, prototype UnitEnum> static public method cases ] {

      - Parameters [0] {
      }
      - Return [ array ]
    }
  }

  - Properties [1] {
    Property [ public protected(set) readonly string $name ]
  }

  - Methods [0] {
  }
}
Constant [ public Suit Wild ] { Object }
Constant [ public Suit H ] { Object }
Class [ <user> class S implements Stringable ] {
  @@ %s 14-14

  - Constants [0] {
  }

  - Static properties [0] {
  }

  - Static methods [0] {
  }

  - Properties [0] {
  }

  - Methods [1] {
    Method [ <user, prototype Stringable> public method __toString ] {
      @@ %s 14 - 14

      - Parameters [0] {
      }
      - Return [ string ]
    }
  }
}
