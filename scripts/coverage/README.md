# Coverage

This repo uses `cargo-llvm-cov` for Rust coverage.

## Install
```bash
cargo install cargo-llvm-cov
```

## Run
```bash
scripts/coverage/coverage.sh
```

Artifacts are written to `artifacts/coverage/`:
- `lcov.info`
- `html/` report
