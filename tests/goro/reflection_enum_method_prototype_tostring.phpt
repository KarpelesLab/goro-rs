--TEST--
Enum cases()/from()/tryFrom() show their UnitEnum/BackedEnum prototype
--FILE--
<?php
echo new ReflectionMethod("RoundingMode", "cases");
enum E: int { case A = 1; }
echo new ReflectionMethod("E", "from");
echo new ReflectionMethod("E", "cases");
echo new ReflectionMethod("Dom\AdjacentPosition", "from");
echo new ReflectionMethod("PropertyHookType", "tryFrom");
--EXPECT--
Method [ <internal:standard, prototype UnitEnum> static public method cases ] {

  - Parameters [0] {
  }
  - Return [ array ]
}
Method [ <internal, prototype BackedEnum> static public method from ] {

  - Parameters [1] {
    Parameter #0 [ <required> string|int $value ]
  }
  - Return [ static ]
}
Method [ <internal, prototype UnitEnum> static public method cases ] {

  - Parameters [0] {
  }
  - Return [ array ]
}
Method [ <internal:dom, prototype BackedEnum> static public method from ] {

  - Parameters [1] {
    Parameter #0 [ <required> string|int $value ]
  }
  - Return [ static ]
}
Method [ <internal:Reflection, prototype BackedEnum> static public method tryFrom ] {

  - Parameters [1] {
    Parameter #0 [ <required> string|int $value ]
  }
  - Return [ ?static ]
}
