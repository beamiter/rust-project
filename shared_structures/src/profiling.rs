// src/profiling.rs
use std::time::{Duration, Instant};

pub struct PerformanceMetrics {
    pub write_latency_ns: Vec<u64>,
    pub read_latency_ns: Vec<u64>,
    pub throughput_msgs_per_sec: f64,
    pub memory_usage_bytes: usize,
}

impl SharedRingBuffer {
    pub fn performance_profile(&self, duration: Duration) -> PerformanceMetrics {
        let mut write_latencies = Vec::new();
        let mut read_latencies = Vec::new();
        let start_time = Instant::now();
        let mut message_count = 0u64;

        let message = SharedMessage::default();

        while start_time.elapsed() < duration {
            // 测量写入延迟
            let write_start = Instant::now();
            if self.try_write_message(&message).unwrap() {
                write_latencies.push(write_start.elapsed().as_nanos() as u64);
                message_count += 1;
            }

            // 测量读取延迟
            let read_start = Instant::now();
            if let Ok(Some(_)) = self.try_read_latest_message() {
                read_latencies.push(read_start.elapsed().as_nanos() as u64);
            }
        }

        let elapsed_secs = start_time.elapsed().as_secs_f64();
        let throughput = message_count as f64 / elapsed_secs;

        PerformanceMetrics {
            write_latency_ns: write_latencies,
            read_latency_ns: read_latencies,
            throughput_msgs_per_sec: throughput,
            memory_usage_bytes: self.shmem.len(),
        }
    }
}
