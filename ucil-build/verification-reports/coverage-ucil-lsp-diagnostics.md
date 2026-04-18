# Coverage Gate — ucil-lsp-diagnostics

- **Verdict**: FAIL
- **Min line coverage**: 85%
- **Min branch coverage**: 75%
- **Generated**: 2026-04-18T22:11:21Z

`cargo llvm-cov report` failed after profraw prune. Tail of log:

```
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2669591-4475060405275049803_23.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2929084-3356260871592694048_28.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2773341-4475060405275049803_29.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2669594-4475060405275049803_26.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2811673-4421149015794932458_25.profraw: invalid instrumentation profile data (file header is corrupt)
warning: /home/rishidarkdevil/Desktop/ucil/target/ucil-2797109-4475060405275049803_21.profraw: invalid instrumentation profile data (file header is corrupt)
error: no profile can be merged
error: failed to merge profile data: process didn't exit successfully: `/home/rishidarkdevil/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/lib/rustlib/x86_64-unknown-linux-gnu/bin/llvm-profdata merge -sparse -f /home/rishidarkdevil/Desktop/ucil/target/ucil-profraw-list -o /home/rishidarkdevil/Desktop/ucil/target/ucil.profdata` (exit status: 1)
```
