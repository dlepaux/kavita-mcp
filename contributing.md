# Contributing

Thanks for helping with kavita-mcp.

## Getting started

```bash
git clone https://github.com/YOUR_USERNAME/kavita-mcp.git
cd kavita-mcp
cargo test
```

The Rust version is pinned in `rust-toolchain.toml`; rustup installs it on the first build. Kavita is
a wiremock server inside the tests, so you need no Kavita, Kindle or network to run them.

Before opening a pull request, run what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
bash scripts/set-version.test.sh
```

## Reporting issues

Search the [existing issues](https://github.com/dlepaux/kavita-mcp/issues) first. One bug per issue,
with what you did, what you expected, what happened, and your kavita-mcp and Kavita versions.

**A security problem is not an issue:** report it privately, as [security.md](security.md) explains.

## Pull requests

- Target `main`, one change per pull request.
- Add a test for a new feature or a bug fix. No test may reach a real Kavita.
- Every sentence a person reads comes from `locales/`, and a new one goes in every locale file. A new
  language needs no Rust beyond one line: see the readme's [Add a language](readme.md#add-a-language).
- A failure the code survives increments a counter as well as logging (`src/metrics.rs`), so it can
  be alerted on.
- Never log a bearer token, a Kavita key or an email address.
- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org) (`feat:`,
  `fix:`, `docs:`…): releases and the changelog are generated from them.

## Code of conduct

This project follows the [code of conduct](code-of-conduct.md).
