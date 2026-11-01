//! Short-lived threads reuse the small-block pools of ended ones: a process
//! starting thousands of them (the test runner does) must not grow by a
//! chunk per thread, which would exhaust a 4 GB address-space cap.

use goro_runtime::mem::CountingAlloc;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// The process's address space size, from /proc.
fn vm_size_kb() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmSize:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn ended_threads_hand_back_their_pools() {
    goro_runtime::mem::limit_system_arenas();
    let run = |n: usize| {
        for i in 0..n {
            std::thread::spawn(move || {
                let v: Vec<Box<[u8; 64]>> = (0..16).map(|_| Box::new([i as u8; 64])).collect();
                std::hint::black_box(v);
            })
            .join()
            .unwrap();
        }
    };
    run(64);
    let before = vm_size_kb();
    // At 256 KB a thread, this would be 512 MB.
    run(2000);
    let grown = vm_size_kb().saturating_sub(before);
    assert!(grown < 64 << 10, "address space grew by {grown} KB");
}
