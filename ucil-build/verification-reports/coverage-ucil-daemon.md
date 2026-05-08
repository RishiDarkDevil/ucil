# Coverage Gate — ucil-daemon

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-08T16:58:49Z

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 15.389500423370023% (floor 85%) |
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
    "count": 579,
    "covered": 72,
    "percent": 12.43523316062176
  },
  "instantiations": {
    "count": 649,
    "covered": 105,
    "percent": 16.178736517719567
  },
  "lines": {
    "count": 4724,
    "covered": 727,
    "percent": 15.389500423370023
  },
  "mcdc": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "regions": {
    "count": 6487,
    "covered": 1051,
    "notcovered": 5436,
    "percent": 16.20163403730538
  }
}
```

## Failures


- Line coverage 15% < floor 85% (delta: 70pp).

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions.
