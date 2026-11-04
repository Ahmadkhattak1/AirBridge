# Contributing

Open a pull request against `main`. CI must pass on Windows and Linux. The maintainer reviews changes to the transfer protocol, radio runtime, installer, tests, and CI configuration.

Run `cargo fmt --all --check`, `cargo test --workspace --locked`, `python scripts/check-test-contract.py`, and `python -m unittest discover -s scripts/tests -v` before requesting review. Linux builds of the desktop UI require the system development libraries listed in `.github/workflows/ci.yml`.

Keep regression coverage for incoming consent, repeated offers, reconnects, archive validation, path traversal, bounded streaming, large videos, and connection lifecycle. Fix a failing test or explain an intentional behavior change in the PR. Do not disable tests or weaken the regression inventory to make CI pass.

Hardware tests need the supported USB adapter and an iPhone. CI cannot prove radio compatibility or transfer speed. Describe device tests separately from automated tests.

For release packaging, see `docs/INSTALLATION.md`. Build artifacts, credentials, private captures, received files, and development distributions must never be committed. Release tags identify tested commits; changes after a release belong in a new version.
