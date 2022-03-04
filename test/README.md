# Test root crate

Organisation of the test code for the `radicle-link` project crates deviates
from the `cargo` conventions in order to work around some of the limitations of
the current `cargo` / Rust testing infrastructure.

Here is how:

- Project crates are set to `test = false` by default, ie. no `#[cfg(test)]` /
  `#[test]` annotated tests are run.

- Instead, project crates are tested via an accompanying `<crate>-test` crate
  located in a `t/` directory relative to the crate root.

- This is similar to what `cargo` calls "integration tests", in that only the
  public API of the crate under test is available. Test crates are, however,
  meant to contain all kinds of tests.

- Conventionally, tests are split into module hierarchies, mainly to support
  convenient filtering.

    `tests`
    : Unit tests. Example-based, preferably-pure.

    `properties`
    : Property tests. Randomized, preferably-pure.

    `integration`
    : Stateful tests, scenario-based. May have all kinds of effects.

- Additionally, test crates may export helpers (such as mocks or fixtures) and
  `proptest` generators through `gen` and `helpers` modules. Test crates may
  depend on each other to make those types / functions available, possibly
  mirroring the dependency relationships of their respective "parent" crates.

- `gen` and `helpers` modules are guarded behind a feature flag "test", ie.

        #[cfg(any(test, feature = "test"))]

- Additional helpers can be found in the `test-helpers` (preferably-pure) and
  `it-helpers` (stateful) crates.

- This crate (`tests`) does not contain any code, but depends on all other test
  crates in the workspace (which are themselves not proper workspace members).
  This prevents unnecessary compilation of test crates if no test target is
  being built, but still makes each test crate available to be executed
  individually via the `-p` flag, eg.

        cargo test -p link-replication-test

- It is recommended to use [`cargo-nextest`](https://nexte.st) instead of `cargo
  test` for maximising parallelism.
