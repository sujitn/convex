# Regression test data

Seven retained JSON fixtures exceed GitHub's normal Git file limit. They are
stored with Git LFS; the repository contains small content-addressed pointers.
The fixture contents, financial inputs, expected results and tolerances are
unchanged. They are test inputs, not production server data.

Before running the full validation suite, install Git LFS and fetch the data:

```sh
git lfs install --local
git lfs pull
PYTHONDONTWRITEBYTECODE=1 python3 tools/assurance/check.py
```

| Fixture under `tests/fixtures/library/` | Required consumer |
| --- | --- |
| `causal-pnl-reference.json` | `causal_pnl_reference.py`, `causal_pnl_ready.py` and `causal_pnl_admitted.py` in CI |
| `concurrent-claims-pnl-reference.json` | Original reference/census checks and the subsequent owned/complete generation checks |
| `concurrent-claims-pnl-owned.json` | Owned census and the complete-generation check |
| `concurrent-claims-pnl-complete.json` | Native/shared regression tests and settled-claims reference generation |
| `settled-claims-pnl-reference.json` | Reference and admitted-generation checks |
| `settled-claims-pnl-admitted.json` | Complete-generation check |
| `settled-claims-pnl-complete.json` | Native/shared regression tests and the complete census |

The relevant CI checkout steps enable LFS before these checks run. Missing data
must fail validation; it must not skip cases or regenerate expected answers.
Other source builds do not need these fixtures.

Private agent harness changes and compiled historical validation binaries are
outside the public PR. Their original repository, branches and evidence are
preserved separately. No private runtime binary is added to the public package.
