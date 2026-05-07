# Coverage Gate — ucil-core

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-07T19:42:13Z

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 4.924760601915184% (floor 85%) |
| Branch       | _unavailable (toolchain)_ |

## Raw JSON

```
{
  "branches": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "functions": {
    "count": 76,
    "covered": 5,
    "percent": 6.578947368421052
  },
  "instantiations": {
    "count": 86,
    "covered": 8,
    "percent": 9.30232558139535
  },
  "lines": {
    "count": 731,
    "covered": 36,
    "percent": 4.924760601915184
  },
  "mcdc": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "regions": {
    "count": 1056,
    "covered": 63,
    "notcovered": 993,
    "percent": 5.965909090909091
  }
}
```

## Failures


- Line coverage 5% < floor 85% (delta: 80pp).

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions.
