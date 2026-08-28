use rtrb::{Consumer, Producer, RingBuffer};

/// Control-thread side of block-boundary activation.
pub struct ActivationController<T> {
    pending: Producer<Box<T>>,
    retired: Consumer<Box<T>>,
}

/// Callback-thread side. Swaps prepared state without allocation or deallocation.
pub struct ActivationBoundary<T> {
    pending: Consumer<Box<T>>,
    retired: Producer<Box<T>>,
    active: Box<T>,
    held_pending: Option<Box<T>>,
}

#[must_use]
pub fn activation_channel<T>(initial: T) -> (ActivationController<T>, ActivationBoundary<T>) {
    let (pending_tx, pending_rx) = RingBuffer::new(1);
    let (retired_tx, retired_rx) = RingBuffer::new(1);
    (
        ActivationController {
            pending: pending_tx,
            retired: retired_rx,
        },
        ActivationBoundary {
            pending: pending_rx,
            retired: retired_tx,
            active: Box::new(initial),
            held_pending: None,
        },
    )
}

impl<T> ActivationController<T> {
    /// Queue a fully prepared graph. Failure leaves the active graph unchanged.
    pub fn prepare(&mut self, prepared: T) -> Result<(), T> {
        self.pending
            .push(Box::new(prepared))
            .map_err(|rtrb::PushError::Full(value)| *value)
    }

    /// Reclaim old graphs away from the callback thread.
    pub fn reclaim_retired(&mut self) -> usize {
        let mut reclaimed = 0;
        while let Ok(retired) = self.retired.pop() {
            drop(retired);
            reclaimed += 1;
        }
        reclaimed
    }
}

impl<T> ActivationBoundary<T> {
    /// Call exactly once at the start of an audio block.
    pub fn activate_at_block_boundary(&mut self) -> bool {
        if self.retired.is_full() {
            return false;
        }
        let next = if let Some(next) = self.held_pending.take() {
            next
        } else {
            let Ok(next) = self.pending.pop() else {
                return false;
            };
            next
        };
        let previous = std::mem::replace(&mut self.active, next);
        if let Err(rtrb::PushError::Full(previous)) = self.retired.push(previous) {
            // Capacity was checked before consuming pending. With this SPSC
            // endpoint no other producer can fill it in between.
            let next = std::mem::replace(&mut self.active, previous);
            self.held_pending = Some(next);
            return false;
        }
        true
    }

    #[must_use]
    pub fn active(&self) -> &T {
        &self.active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_graph_swaps_only_at_boundary_and_reclaims_off_callback() {
        let (mut controller, mut boundary) = activation_channel("last-good");
        controller.prepare("next").unwrap();
        assert_eq!(boundary.active(), &"last-good");
        assert!(boundary.activate_at_block_boundary());
        assert_eq!(boundary.active(), &"next");
        assert_eq!(controller.reclaim_retired(), 1);
    }

    #[test]
    fn full_pending_slot_preserves_last_good() {
        let (mut controller, boundary) = activation_channel(1);
        controller.prepare(2).unwrap();
        assert_eq!(controller.prepare(3), Err(3));
        assert_eq!(boundary.active(), &1);
    }

    #[test]
    fn second_activation_waits_for_off_callback_reclamation() {
        let (mut controller, mut boundary) = activation_channel(1);
        controller.prepare(2).unwrap();
        assert!(boundary.activate_at_block_boundary());
        controller.prepare(3).unwrap();

        assert!(!boundary.activate_at_block_boundary());
        assert_eq!(boundary.active(), &2);
        assert_eq!(controller.reclaim_retired(), 1);
        assert!(boundary.activate_at_block_boundary());
        assert_eq!(boundary.active(), &3);
    }
}
