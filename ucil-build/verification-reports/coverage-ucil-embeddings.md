# Coverage Gate — ucil-embeddings

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-06T18:54:04Z

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 80% (floor 85%) |
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
    "count": 20,
    "covered": 14,
    "percent": 70
  },
  "instantiations": {
    "count": 20,
    "covered": 14,
    "percent": 70
  },
  "lines": {
    "count": 195,
    "covered": 156,
    "percent": 80
  },
  "mcdc": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "regions": {
    "count": 321,
    "covered": 278,
    "notcovered": 43,
    "percent": 86.60436137071652
  }
}
```

## Failures


- Line coverage 80% < floor 85% (delta: 5pp).

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions.
