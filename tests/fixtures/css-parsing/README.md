# CSS token fixtures

`component_value_list.json` and `LICENSE` are unchanged copies from
`servo/rust-cssparser`, revision
`9f3ceddf398f5ce4ab76fdad1b42ccb6c578a926`, under `src/css-parsing-tests/`.
The fixtures are dedicated to the public domain under CC0.

Source: https://github.com/servo/rust-cssparser/tree/9f3ceddf398f5ce4ab76fdad1b42ccb6c578a926/src/css-parsing-tests

The integration harness groups flat tokens into the fixture's component-value
notation without using the production rule parser. Number representations in
these fixtures are canonical serializer spellings, not original lexemes; the
harness compares numeric values and type flags instead. Original spellings and
signs are tested separately. Legacy match operators (`~=`, `|=`, `^=`, `$=`,
`*=`) are represented by two delimiter tokens in current CSS Syntax.

One expectation from the 2013 draft is explicitly updated in the harness:
U+0080 and U+0081 are delimiters rather than identifier characters under the
current specification's non-ASCII ident code point ranges. The original fixture
and its raw result remain visible in that test. No cases are skipped.
