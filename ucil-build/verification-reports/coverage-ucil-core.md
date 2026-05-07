# Coverage Gate — ucil-core

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-07T22:28:32Z

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 4.008908685968819% (floor 85%) |
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
    "count": 100,
    "covered": 5,
    "percent": 5
  },
  "instantiations": {
    "count": 110,
    "covered": 8,
    "percent": 7.272727272727272
  },
  "lines": {
    "count": 898,
    "covered": 36,
    "percent": 4.008908685968819
  },
  "mcdc": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "regions": {
    "count": 1276,
    "covered": 63,
    "notcovered": 1213,
    "percent": 4.93730407523511
  }
}
```

## Failures


- Line coverage 4% < floor 85% (delta: 81pp).

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions.
