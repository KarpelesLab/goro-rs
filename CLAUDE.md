# Instructions for AI agents

Read [README.md](README.md) and [CLEANROOM.md](CLEANROOM.md) first. The
clean-room policy is the project's top rule, and every agent prompt must
restate it, including prompts for sub-agents.

## Clean room

- Never open, read, grep, port or generate from php-src source, the source of
  any library PHP uses, PECL source, or this repository's `old` branch.
- Write from scratch. Don't translate goro (the Go implementation) either.
- To learn a behaviour: read the PHP manual or RFC, read a `.phpt` test's
  expected output, or run a script on the reference PHP binary.
- Cite behaviour sources in commits and comments ("PHP manual: …", "test
  Zend/tests/foo.phpt", "observed on PHP 8.5"), never php-src internals.

## This machine

The development machine is the maintainer's desktop, shared with other work.

- Work only inside this repository, its worktrees, your scratch directory and
  build directories, and the test-suite checkout. Never scan the filesystem
  (no `find`, `bfs`, `du` or similar over `/`, `/home`, `/tmp`, `/usr`) and
  never use files from the system or the user's home as data.
- Builds: at most one at a time across agents, through
  `flock /tmp/claude-1000/goro-build.lock cargo build ...`. Keep the job
  count from `~/.cargo/config.toml` (don't pass `-j`). An agent working in its
  own worktree uses its own `CARGO_TARGET_DIR=/tmp/cargo-target-goro-<name>`
  and deletes it when done.
- Don't let a hang hold the build lock: run tests under `timeout` (e.g.
  `flock ... timeout 900 cargo test ...`), or build under the lock with
  `--no-run` and run the test binaries outside it.
- Memory: every goro process must be memory-capped. `goro --test` caps
  each test (RLIMIT_AS) and goro enforces `memory_limit` (4 GB backstop when
  unlimited). Run any goro or php85 script outside the runner under
  `prlimit --as=4294967296 timeout 120 ...`. Never keep old goro binary
  copies (baselines) that predate the memory_limit enforcement. On
  2026-10-29 uncapped goro test processes reached 30–110 GB and the kernel
  OOM killer killed the maintainer's applications.
- Full-suite runs (`goro --test` over a whole tree): one at a time across
  all agents, through `flock /tmp/claude-1000/goro-suite.lock`.
- Tests and benchmarks: at most about 8 parallel jobs. Check `free -g` first,
  and wait if less than 30 GB is available or the machine is swapping.
- Containers (the reference PHP binary runs in one): spot checks only, batched
  into one invocation where possible. Never run a test suite or a fuzz batch
  through containers, and never leave one running.
- At most one sub-agent per agent.
- Delete your worktrees, scratch files and build directories when done.
- Stop processes only by PID, and only ones you started.

## Test suite

php-src's tests (only the tests) are at `/tmp/claude-1000/php-tests`
(php-src 8.5.4: `tests/`, `Zend/tests/`, `ext/*/tests/`, `sapi/*/tests/`).
Never fetch other php-src files.

The reference PHP 8.5 binary is `/tmp/claude-1000/bin/php85` (a container
wrapper). It loads a production php.ini, so pass
`-d error_reporting=-1 -d display_errors=1 -d log_errors=0 -d html_errors=0`
for comparable output. Spot checks only.

## Code

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  must pass before each push.
- `Cargo.lock` is not committed.
