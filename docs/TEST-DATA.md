# Regression test data

The financial regression corpora under `tests/fixtures/library/` total about
5.9 GB of JSON, mostly near-identical books. Every file of 1 MB or more is
committed as `<name>.json.xz` (about 14 MB in all); the archives are the
source of truth.
The fixture contents, financial inputs, expected results and tolerances are
unchanged. They are test inputs, not production server data.

Before building tests, examples or bindings, or running the validation
scripts, materialize the JSON:

```sh
python3 tools/fixtures.py materialize
PYTHONDONTWRITEBYTECODE=1 python3 tools/assurance/check.py
```

`materialize` writes each `<name>.json` beside its archive, where Git ignores
it (`tests/fixtures/.gitignore`), and rewrites a stale copy from its archive.
xz's own CRC64 check rejects a corrupt archive. Rust tests read these files at run time through `tests/fixtures/packed.rs`
instead of embedding them with `include_str!`. A missing file fails the test
with an instruction to run `materialize`; it never skips cases or regenerates
expected answers.

After deliberately regenerating a packed fixture with its `tests/scripts`
writer, refresh its archive and ignore entry:

```sh
python3 tools/fixtures.py pack tests/fixtures/library/<name>.json
python3 tools/fixtures.py check
```

The writers are byte-reproducible on Linux (glibc). On Windows, MSVC's libm
changes the last digit of some floating-point results, so regenerate on Linux.

Private agent harness changes and compiled historical validation binaries are
outside the public PR. Their original repository, branches and evidence are
preserved separately. No private runtime binary is added to the public package.
