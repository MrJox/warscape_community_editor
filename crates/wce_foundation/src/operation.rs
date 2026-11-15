//! A document-local lease for its native Save/Export routes.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[derive(Default, Debug)]
pub struct Gate(Arc<AtomicBool>);
pub struct Lease(Arc<AtomicBool>);
impl Gate {
    pub fn busy(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub fn acquire(&self) -> Result<Lease, String> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Busy: this document is already saving or exporting".to_owned())?;
        Ok(Lease(self.0.clone()))
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
