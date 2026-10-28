# Instructions for AI agents

Read [README.md](README.md) and [CLEANROOM.md](CLEANROOM.md) first. The
clean-room policy is the project's top rule, and every agent prompt must
restate it, including prompts for sub-agents.

## Clean room

- Never open, read, grep, port or generate from php-src source, the source of
  any library PHP uses, PECL source, or this repository's `old` branch.
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
  `flock /tmp/claude-1000/goro-build.lock cargo build --release -j4`.
- Tests and benchmarks: at most about 8 parallel jobs. Check `free -g` first,
  and wait if less than 30 GB is available or the machine is swapping.
- Containers (the reference PHP binary runs in one): spot checks only, batched
  into one invocation where possible. Never run a test suite or a fuzz batch
  through containers, and never leave one running.
- At most one sub-agent per agent.
- Delete your worktrees, scratch files and build directories when done.
- Stop processes only by PID, and only ones you started.

## Code

- `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  must pass before each push.
- `Cargo.lock` is not committed.
