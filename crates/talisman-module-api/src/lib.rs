pub use talisman_signals::{ControlMsg, ControlSignal, Manifest, PortDesc};

/// A restricted context for RT-safe tick operations.
/// No allocation allowed here for Tier 0 modules.
pub struct TickCx<'a> {
    /// Timestamp (frames since start)
    pub frame: u64,
    /// Delta time in seconds
    pub dt: f64,
    /// Placeholder for output queue / signal bus access
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> TickCx<'a> {
    pub fn new(frame: u64, dt: f64) -> Self {
        Self {
            frame,
            dt,
            _marker: std::marker::PhantomData,
        }
    }
}

/// A context for control-plane operations (non-RT).
/// Allocations and complex logic allowed here.
pub struct ControlCx {
    // Placeholder for host callbacks (logging, resizing, etc.)
}

impl ControlCx {
    pub fn new() -> Self {
        Self {}
    }
}

/// The trait that all Static (Tier 0/1) modules must implement.
/// This runs inside the host process.
pub trait StaticModule: Send {
    /// Static metadata about the module
    fn manifest(&self) -> Manifest;

    /// Declare input/output ports
    fn ports(&self) -> &[PortDesc];

    /// Called on non-RT thread (control plane)
    fn on_control(&mut self, cx: &mut ControlCx, msg: ControlMsg);

    /// Called on the module’s scheduler context.
    /// For RT modules, host guarantees this is RT-safe and allocation-free.
    fn tick(&mut self, cx: &mut TickCx);
}
