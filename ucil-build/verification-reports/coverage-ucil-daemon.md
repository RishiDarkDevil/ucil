# Coverage Gate — ucil-daemon

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-04-18T10:44:28Z

`cargo llvm-cov report` failed after profraw prune. Tail of log:

```
warning: /home/rishidarkdevil/Desktop/ucil-wt/WO-0027/target/WO-0027-985646-16103188886603022107_14.profraw: invalid instrumentation profile data (file header is corrupt)
error: no profile can be merged
error: failed to merge profile data: process didn't exit successfully: `/home/rishidarkdevil/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata merge -sparse -f /home/rishidarkdevil/Desktop/ucil-wt/WO-0027/target/WO-0027-profraw-list -o /home/rishidarkdevil/Desktop/ucil-wt/WO-0027/target/WO-0027.profdata` (exit status: 1)
```
