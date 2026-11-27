# Dependents report: parse

Real projects' own test suites, run against the original library and against the port. This is a report, not a gate. A test is flagged only if it passed in every run with the original and failed with the port.

- **Date:** 2026-11-27
- **Original:** https://github.com/r1chardj0n3s/parse at `7f07b64089338ee7b7db85a752eb45c4816c2657`
- **Port:** `ports/parse` at Portwright commit `df41a90`, wheel `parse_rust_port-0.1.0-cp312-cp312-manylinux_2_34_x86_64.whl` (sha256 `1577e06fbbf2c35d`)
- **Python:** 3.12.14

| Dependent | Version | Tests | Pass with original | Fail with original | Flaky with original | Flagged |
| --- | --- | --- | --- | --- | --- | --- |
| parse_type | 0.6.6 (`5c4126ae39`) | 334 | 331 | 1 | 0 | 13 |
| pytest-bdd | 9.0.1 (`aad3bf4350`) | 146 | 145 | 0 | 0 | 3 |

## parse_type 0.6.6

- `original-1`: 331 passed, 2 skipped, 1 failed; pytest exit 1
- `original-2`: 331 passed, 2 skipped, 1 failed; pytest exit 1
- `port-1`: 318 passed, 2 skipped, 14 failed; pytest exit 1

Failing with the original too (not flagged): `tests.parse_tests.test_pattern::test_numbered`

### Flagged

- `tests.test_cfparse.TestParser::test_create_missing_types__if_special_types_are_missing` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_cfparse.TestParser testMethod=test_create_missing_types__if_special_types_are_missing>
      def test_create_missing_types__if_special_types_are_missing(self):
          existing_types = dict(Number=parse_number)
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_cfparse.TestParser::test_create_missing_types__if_special_types_exist` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_cfparse.TestParser testMethod=test_create_missing_types__if_special_types_exist>
      def test_create_missing_types__if_special_types_exist(self):
          existing_types = dict(Number=parse_number)
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_cfparse.TestParser::test_create_missing_types__raises_error_if_primary_type_is_missing` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_cfparse.TestParser testMethod=test_create_missing_types__raises_error_if_primary_type_is_missing>
      def test_create_missing_types__raises_error_if_primary_type_is_missing(self):
          # -- HINT: primary type is not provided in type_dict (existing_types)
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_cfparse.TestParser::test_create_missing_types__without_cardinality_fields_in_schema` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_cfparse.TestParser testMethod=test_create_missing_types__without_cardinality_fields_in_schema>
      def test_create_missing_types__without_cardinality_fields_in_schema(self):
          schemas = ["{}", "{:Number}", "{number3}", "{number4:Number}", "XXX"]
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_cfparse.TestParser::test_parser__can_parse_cardinality_field_many` (failed with the port)

  ```
  ValueError: format spec 'Number+' not recognised
  self = <tests.test_cfparse.TestParser testMethod=test_parser__can_parse_cardinality_field_many>
      def test_parser__can_parse_cardinality_field_many(self):
          # -- CARDINALITY: 1..* = one_or_more = many
          existing_types = dict(Number=parse_number)
          self.assertFalse("Number+" in existing_types)
          # -- ENSURE: Missing type variant is created.
          schema = "List: {numbers:Number+}"
  >       parser = Parser(schema, existing_types)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  E       ValueError: format spec 'Number+' not recognised
  tests/test_cfparse.py:72: ValueError
  ```
- `tests.test_cfparse.TestParser::test_parser__can_parse_cardinality_field_many0` (failed with the port)

  ```
  ValueError: format spec 'Number*' not recognised
  self = <tests.test_cfparse.TestParser testMethod=test_parser__can_parse_cardinality_field_many0>
      def test_parser__can_parse_cardinality_field_many0(self):
          # -- CARDINALITY: 0..* = zero_or_more = many0
          existing_types = dict(Number=parse_number)
          self.assertFalse("Number*" in existing_types)
          # -- ENSURE: Missing type variant is created.
          schema = "List0: {numbers:Number*}"
  >       parser = Parser(schema, existing_types)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  E       ValueError: format spec 'Number*' not recognised
  tests/test_cfparse.py:115: ValueError
  ```
- `tests.test_cfparse.TestParser::test_parser__can_parse_cardinality_field_many_with_own_type_builder` (failed with the port)

  ```
  TypeError: Parser.__new__() got an unexpected keyword argument 'type_builder'
  self = <tests.test_cfparse.TestParser testMethod=test_parser__can_parse_cardinality_field_many_with_own_type_builder>
      def test_parser__can_parse_cardinality_field_many_with_own_type_builder(self):
          # -- CARDINALITY: 1..* = one_or_more = many
  ...
          self.assertFalse("Number+" in existing_types)
          # -- ENSURE: Missing type variant is created.
          schema = "List: {numbers:Number+}"
  >       parser = Parser(schema, existing_types, type_builder=type_builder)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  E       TypeError: Parser.__new__() got an unexpected keyword argument 'type_builder'
  tests/test_cfparse.py:95: TypeError
  ```
- `tests.test_cfparse.TestParser::test_parser__can_parse_cardinality_field_optional` (failed with the port)

  ```
  ValueError: format spec 'Number?' not recognised
  self = <tests.test_cfparse.TestParser testMethod=test_parser__can_parse_cardinality_field_optional>
      def test_parser__can_parse_cardinality_field_optional(self):
          # -- CARDINALITY: 0..1 = zero_or_one = optional
          existing_types = dict(Number=parse_number)
          self.assertFalse("Number?" in existing_types)
          # -- ENSURE: Missing type variant is created.
          schema = "OptionalNumber: {number:Number?}"
  >       parser = Parser(schema, existing_types)
                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  E       ValueError: format spec 'Number?' not recognised
  tests/test_cfparse.py:54: ValueError
  ```
- `tests.test_cfparse.TestParser::test_parser__can_parse_normal_fields` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_cfparse.TestParser testMethod=test_parser__can_parse_normal_fields>
      def test_parser__can_parse_normal_fields(self):
          existing_types = dict(Number=parse_number)
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_parse_util.TestFieldParser::test_extract_fields__with_many_fields` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_parse_util.TestFieldParser testMethod=test_extract_fields__with_many_fields>
      def test_extract_fields__with_many_fields(self):
          MANY_FIELDS_DATA = [
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_parse_util.TestFieldParser::test_extract_fields__with_one_field` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_parse_util.TestFieldParser testMethod=test_extract_fields__with_one_field>
      def test_extract_fields__with_one_field(self):
          prefix = "XXX ___"
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_parse_util.TestFieldParser::test_extract_fields__without_field` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_parse_util.TestFieldParser testMethod=test_extract_fields__without_field>
      def test_extract_fields__without_field(self):
          prefix = "XXX ___"
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```
- `tests.test_parse_util.TestFieldParser::test_extract_types` (failed with the port)

  ```
  AttributeError: module 'parse' has no attribute 'PARSE_RE'
  self = <tests.test_parse_util.TestFieldParser testMethod=test_extract_types>
      def test_extract_types(self):
          MANY_TYPES_DATA = [
  ...
          :return: Generator for fields in schema (as Field objects).
          """
          # -- BASED-ON: parse.Parser._generate_expression()
  >       for part in parse.PARSE_RE.split(schema):
                      ^^^^^^^^^^^^^^
  E       AttributeError: module 'parse' has no attribute 'PARSE_RE'
  parse_type/parse_util.py:182: AttributeError
  ```

## pytest-bdd 9.0.1

- `original-1`: 145 passed, 1 skipped; pytest exit 0
- `original-2`: 145 passed, 1 skipped; pytest exit 0
- `port-1`: 142 passed, 1 skipped, 3 failed; pytest exit 1

### Flagged

- `tests.args.cfparse.test_args::test_argument_in_when` (failed with the port)

  ```
  AssertionError: assert {'passed': 0,...rors': 1, ...} == {'passed': 1,...rors': 0, ...}
    Omitting 4 identical items, use -vv to show
    Differing items:
    {'errors': 1} != {'errors': 0}
  ...
  E         
  E         Omitting 4 identical items, use -vv to show
  E         Differing items:
  E         {'errors': 1} != {'errors': 0}
  E         {'passed': 0} != {'passed': 1}
  E         Use -v to get more diff
  /home/user/portwright/target/dependents/runs/pytest-bdd/port-1/tree/tests/args/cfparse/test_args.py:111: AssertionError
  ```
- `tests.args.cfparse.test_args::test_every_step_takes_param_with_the_same_name` (failed with the port)

  ```
  AssertionError: assert {'passed': 0,...rors': 1, ...} == {'passed': 1,...rors': 0, ...}
    Omitting 4 identical items, use -vv to show
    Differing items:
    {'errors': 1} != {'errors': 0}
  ...
  E         
  E         Omitting 4 identical items, use -vv to show
  E         Differing items:
  E         {'errors': 1} != {'errors': 0}
  E         {'passed': 0} != {'passed': 1}
  E         Use -v to get more diff
  /home/user/portwright/target/dependents/runs/pytest-bdd/port-1/tree/tests/args/cfparse/test_args.py:59: AssertionError
  ```
- `tests.generation.test_generate_missing::test_generate_missing_with_step_parsers` (failed with the port)

  ```
  AssertionError: assert {'passed': 0,...rors': 1, ...} == {'passed': 0,...rors': 0, ...}
    Omitting 5 identical items, use -vv to show
    Differing items:
    {'errors': 1} != {'errors': 0}
  ...
  E       AssertionError: assert {'passed': 0,...rors': 1, ...} == {'passed': 0,...rors': 0, ...}
  E         
  E         Omitting 5 identical items, use -vv to show
  E         Differing items:
  E         {'errors': 1} != {'errors': 0}
  E         Use -v to get more diff
  /home/user/portwright/target/dependents/runs/pytest-bdd/port-1/tree/tests/generation/test_generate_missing.py:138: AssertionError
  ```
