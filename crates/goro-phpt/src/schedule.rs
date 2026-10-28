//! Parallel execution honoring CONFLICTS.
//!
//! Documentation of CONFLICTS: one key per line, `#` comments allowed; tests
//! sharing a key don't run at the same time, and the key `all` keeps a test
//! from running alongside any other. A file named CONFLICTS in a test's
//! directory applies to every test there. Tests with `all` run after the
//! others, one at a time. A REDIRECTTEST runs other tests' files, so it is
//! treated as `all` too.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};

use crate::section::{Phpt, list_lines};

#[derive(Debug, Clone)]
pub struct Job {
    pub path: PathBuf,
    pub conflicts: Vec<String>,
}

impl Job {
    pub fn is_exclusive(&self) -> bool {
        self.conflicts.iter().any(|c| c == "all")
    }
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Builds jobs for `paths`, reading their CONFLICTS.
pub fn jobs(paths: &[PathBuf]) -> Vec<Job> {
    let mut dir_cache: HashMap<PathBuf, Vec<String>> = HashMap::new();
    paths
        .iter()
        .map(|p| {
            let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
            let mut conflicts = dir_cache
                .entry(dir.clone())
                .or_insert_with(|| {
                    std::fs::read(dir.join("CONFLICTS"))
                        .map(|b| list_lines(&b, true))
                        .unwrap_or_default()
                })
                .clone();
            if let Ok(data) = std::fs::read(p) {
                let has_conflicts = find(&data, b"\n--CONFLICTS--");
                let redirect = find(&data, b"\n--REDIRECTTEST--");
                if (has_conflicts || redirect)
                    && let Ok(phpt) = Phpt::parse_sections(&data)
                {
                    conflicts.extend(phpt.get_list("CONFLICTS", true));
                    if phpt.has("REDIRECTTEST") {
                        conflicts.push("all".into());
                    }
                }
            }
            conflicts.sort();
            conflicts.dedup();
            Job {
                path: p.clone(),
                conflicts,
            }
        })
        .collect()
}

struct State {
    free: VecDeque<usize>,
    keyed: Vec<usize>,
    held: HashSet<String>,
}

/// Runs `f` on every job with `workers` threads.
pub fn run_all<F>(jobs: &[Job], workers: usize, f: F)
where
    F: Fn(&Job) + Sync,
{
    let mut free = VecDeque::new();
    let mut keyed = Vec::new();
    let mut exclusive = Vec::new();
    for (i, j) in jobs.iter().enumerate() {
        if j.is_exclusive() {
            exclusive.push(i);
        } else if j.conflicts.is_empty() {
            free.push_back(i);
        } else {
            keyed.push(i);
        }
    }
    let state = Mutex::new(State {
        free,
        keyed,
        held: HashSet::new(),
    });
    let cv = Condvar::new();
    std::thread::scope(|s| {
        for _ in 0..workers.max(1) {
            s.spawn(|| {
                loop {
                    let idx = {
                        let mut st = state.lock().unwrap();
                        loop {
                            let pos = st
                                .keyed
                                .iter()
                                .position(|&i| jobs[i].conflicts.iter().all(|c| !st.held.contains(c)));
                            if let Some(pos) = pos {
                                let i = st.keyed.remove(pos);
                                for c in &jobs[i].conflicts {
                                    st.held.insert(c.clone());
                                }
                                break Some(i);
                            }
                            if let Some(i) = st.free.pop_front() {
                                break Some(i);
                            }
                            if st.keyed.is_empty() {
                                break None;
                            }
                            st = cv.wait(st).unwrap();
                        }
                    };
                    let Some(i) = idx else { break };
                    f(&jobs[i]);
                    if !jobs[i].conflicts.is_empty() {
                        let mut st = state.lock().unwrap();
                        for c in &jobs[i].conflicts {
                            st.held.remove(c);
                        }
                        cv.notify_all();
                    }
                }
            });
        }
    });
    for i in exclusive {
        f(&jobs[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn conflicting_jobs_never_overlap() {
        let mk = |n: usize, c: &[&str]| Job {
            path: PathBuf::from(format!("t{n}.phpt")),
            conflicts: c.iter().map(|s| s.to_string()).collect(),
        };
        let mut jobs = Vec::new();
        for n in 0..40 {
            jobs.push(match n % 4 {
                0 => mk(n, &["db"]),
                1 => mk(n, &[]),
                2 => mk(n, &["db", "server"]),
                _ => mk(n, &["all"]),
            });
        }
        let running_db = AtomicUsize::new(0);
        let running = AtomicUsize::new(0);
        let done = AtomicUsize::new(0);
        let bad = AtomicUsize::new(0);
        run_all(&jobs, 6, |j| {
            let r = running.fetch_add(1, Ordering::SeqCst);
            if j.is_exclusive() && r != 0 {
                bad.fetch_add(1, Ordering::SeqCst);
            }
            let db = j.conflicts.iter().any(|c| c == "db");
            if db && running_db.fetch_add(1, Ordering::SeqCst) != 0 {
                bad.fetch_add(1, Ordering::SeqCst);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
            if db {
                running_db.fetch_sub(1, Ordering::SeqCst);
            }
            running.fetch_sub(1, Ordering::SeqCst);
            done.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(done.load(Ordering::SeqCst), 40);
        assert_eq!(bad.load(Ordering::SeqCst), 0);
    }
}
