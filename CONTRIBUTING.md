# Contributing

Use neutral examples and synthetic data. Never commit machine names, account
names, network addresses, SSH material, Codex task identifiers, authentication,
or raw user logs.

Before submitting a change, run:

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
python3 -m unittest discover -s tests -p 'test_*.py'
sh -n install.sh uninstall.sh scripts/*.sh
./scripts/privacy-scan.sh --root . --history
```

Changes to lifecycle behavior require a regression test that proves both the
cleanup and the active-turn safety boundary.
