/// Fiber state machine states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberState {
    Created = 0,
    Running = 1,
    Suspended = 2,
    Terminated = 3,
}

/// The sentinel error message used to signal fiber suspension
pub const FIBER_SUSPEND_SENTINEL: &str = "__FIBER_SUSPEND__";
