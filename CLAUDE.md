# Claude instructions

This project follows [REAS](https://github.com/abenamer/reas). Start at
[docs/index.md](docs/index.md) and read only what the task needs.

1. Keep domain rules in `src/domain/` free of I/O and frameworks.
2. Add a port only where a second implementation or a test double exists.
3. Construct adapters in `src/composition.rs`, nowhere else.
4. Return typed errors. Panic only for a bug.
5. Run CPU work off the executor (`spawn_blocking`) in async code.
6. After each step run `cargo fmt --all`, `cargo check --all-targets`,
   `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
7. Update `docs/` in the same change as the code; run
   `python scripts/check_docs.py` and `python -m unittest discover -s tests`.
8. Record evidence in `docs/self-audit.md`: what ran, when, on which revision.
9. Do not claim passing checks or STE compliance without evidence.
10. Keep `data/programs.json` compatible with the Python original's shape.
11. Work through pull requests into `main` with a code review and a
    security review on each. Commits may carry a `Co-Authored-By` trailer
    but never a `Claude-Session` trailer, and pull requests and comments
    never link to a Claude session: the link works only for its owner.
