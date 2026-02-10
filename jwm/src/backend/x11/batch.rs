/// X11 Request Batching - Reduces flush() calls for better performance
///
/// Instead of flushing after every configure/property operation,
/// batch operations and flush periodically to reduce X11 round-trips.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::Mutex;
use x11rb::connection::Connection;
use crate::backend::error::BackendError;

/// Batches X11 requests and flushes intelligently
pub struct X11RequestBatcher {
    /// Count of pending operations
    pending_ops: Arc<AtomicU32>,
    /// Last flush time
    last_flush: Arc<Mutex<Instant>>,
    /// Threshold: flush after N operations OR M milliseconds
    flush_op_threshold: u32,
    flush_time_threshold_ms: u64,
}

impl X11RequestBatcher {
    pub fn new() -> Self {
        Self {
            pending_ops: Arc::new(AtomicU32::new(0)),
            last_flush: Arc::new(Mutex::new(Instant::now())),
            flush_op_threshold: 8,     // Flush after 8 queued operations
            flush_time_threshold_ms: 8, // OR after 8ms
        }
    }

    /// Record an operation and maybe flush
    pub fn mark_op<C: Connection>(&self, conn: &C) -> Result<(), BackendError> {
        let count = self.pending_ops.fetch_add(1, Ordering::SeqCst);

        // Check if we should flush
        let should_flush = if count > 0 && count % self.flush_op_threshold == 0 {
            // Operation threshold reached
            true
        } else if let Ok(last) = self.last_flush.lock() {
            // Time threshold reached?
            last.elapsed() > Duration::from_millis(self.flush_time_threshold_ms)
        } else {
            false
        };

        if should_flush {
            self.do_flush(conn)?;
        }

        Ok(())
    }

    /// Force a flush
    pub fn flush<C: Connection>(&self, conn: &C) -> Result<(), BackendError> {
        self.do_flush(conn)?;
        Ok(())
    }

    fn do_flush<C: Connection>(&self, conn: &C) -> Result<(), BackendError> {
        conn.flush()?;
        self.pending_ops.store(0, Ordering::SeqCst);
        if let Ok(mut last) = self.last_flush.lock() {
            *last = Instant::now();
        }
        Ok(())
    }

    /// Get pending operations count (for debugging)
    #[allow(dead_code)]
    pub fn pending_count(&self) -> u32 {
        self.pending_ops.load(Ordering::Acquire)
    }
}

impl Clone for X11RequestBatcher {
    fn clone(&self) -> Self {
        Self {
            pending_ops: self.pending_ops.clone(),
            last_flush: self.last_flush.clone(),
            flush_op_threshold: self.flush_op_threshold,
            flush_time_threshold_ms: self.flush_time_threshold_ms,
        }
    }
}

impl Default for X11RequestBatcher {
    fn default() -> Self {
        Self::new()
    }
}
