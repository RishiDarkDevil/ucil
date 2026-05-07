# Coverage Gate — ucil-agents

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-05-07T17:49:44Z

## Summary

| Metric       | Value |
|--------------|-------|
| Line         | 0% (floor 85%) |
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
    "count": 0,
    "covered": 0,
    "percent": 0
  },
  "instantiations": {
    "count": 0,
    "covered": 0,
    "percent": 0
  },
  "lines": {
    "count": 0,
    "covered": 0,
    "percent": 0
  },
  "mcdc": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  },
  "regions": {
    "count": 0,
    "covered": 0,
    "notcovered": 0,
    "percent": 0
  }
}
```

## Failures


- Line coverage 0% < floor 85% (delta: 85pp).

## Why this is failing

Coverage below the floor means code paths exist that no test ever
exercises. Combine this with mutation-gate: if a new file has 95% line
coverage but 40% mutation score, the tests run the lines without
asserting on their effects. Address both dimensions.
