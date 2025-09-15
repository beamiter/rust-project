// benches/ring_buffer_bench.rs
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

// 根据你的工程路径
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};

// 统一工具
fn mk_path(name: &str) -> String {
    format!("/tmp/{}_{}", name, std::process::id())
}

fn drain_all(buffer: &SharedRingBuffer) {
    while let Ok(Some(_)) = buffer.try_read_next_message() {}
}

fn create_test_message(id: i32) -> SharedMessage {
    let mut message = SharedMessage::default();
    message.get_monitor_info_mut().monitor_num = id;
    message
        .get_monitor_info_mut()
        .set_client_name(&format!("test_client_{}", id));
    message.get_monitor_info_mut().set_ltsymbol("[]=");
    message
}

fn prebuild_messages(count: usize, base_id: i32) -> Vec<SharedMessage> {
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        v.push(create_test_message(base_id + i as i32));
    }
    v
}

// 1) 单线程写入
fn bench_single_threaded_write(c: &mut Criterion) {
    let test_path = mk_path("bench_single_write");
    let _ = std::fs::remove_file(&test_path);

    let buffer = SharedRingBuffer::create(&test_path, Some(1024), Some(0)).unwrap();
    let messages = prebuild_messages(100, 0);

    c.bench_function("single_threaded_write", |b| {
        b.iter(|| {
            drain_all(&buffer);
            for m in &messages {
                while !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                    let _ = buffer.try_read_next_message();
                }
            }
            black_box(buffer.available_messages());
        })
    });

    drop(buffer);
    let _ = std::fs::remove_file(&test_path);
}

// 2) 单线程读取
fn bench_single_threaded_read(c: &mut Criterion) {
    let test_path = mk_path("bench_single_read");
    let _ = std::fs::remove_file(&test_path);

    let buffer = SharedRingBuffer::create(&test_path, Some(1024), Some(0)).unwrap();
    let messages = prebuild_messages(100, 10_000);

    c.bench_function("single_threaded_read", |b| {
        b.iter_batched(
            || {
                drain_all(&buffer);
                for m in &messages {
                    while !buffer.try_write_message(m).unwrap_or(false) {
                        let _ = buffer.try_read_next_message();
                    }
                }
            },
            |_| {
                while let Ok(Some(_)) = buffer.try_read_next_message() {}
            },
            BatchSize::SmallInput,
        )
    });

    drop(buffer);
    let _ = std::fs::remove_file(&test_path);
}

// 3) 写吞吐（不同消息数）
fn bench_throughput_varying_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_by_message_count");

    for &count in &[10usize, 100, 1000, 10_000] {
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::new("write_messages", count),
            &count,
            |b, &count| {
                let test_path = mk_path(&format!("bench_throughput_{}", count));
                let _ = std::fs::remove_file(&test_path);

                let buffer = SharedRingBuffer::create(&test_path, Some(16_384), Some(0)).unwrap();
                let messages = prebuild_messages(count, 20_000);

                b.iter(|| {
                    drain_all(&buffer);
                    for m in &messages {
                        while !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                            let _ = buffer.try_read_next_message();
                        }
                    }
                });

                drop(buffer);
                let _ = std::fs::remove_file(&test_path);
            },
        );
    }
    group.finish();
}

// 4) 生产者-消费者（复用段与线程：一次样本内只创建一次）
// 修复点：
// - 不再让生产者在环满时调用 try_read_next_message（严格 SPSC）
// - 不再按“每轮读满 message_count”退出；改为按总目标条目数收敛（iters * message_count）
// - 用原子计数 sent/received 统计总量，消费者持续 wait+drain 直到达到目标
fn bench_producer_consumer(c: &mut Criterion) {
    let mut group = c.benchmark_group("producer_consumer");
    group.sample_size(10);

    for &spins in &[0u32, 1000, 5000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("adaptive_polling", spins),
            &spins,
            |b, &spins| {
                let test_path = mk_path(&format!("bench_pc_{}", spins));
                let _ = std::fs::remove_file(&test_path);

                b.iter_custom(|iters| {
                    // 构建共享段（生产者创建，消费者打开）
                    let producer = Arc::new(
                        SharedRingBuffer::create(&test_path, Some(2048), Some(spins)).unwrap(),
                    );
                    thread::sleep(Duration::from_millis(5));
                    let consumer =
                        Arc::new(SharedRingBuffer::open(&test_path, Some(spins)).unwrap());

                    // 一次样本内的固定总工作量
                    let message_count_per_round = 1000usize;
                    let total_to_send = (iters as usize) * message_count_per_round;

                    // 预构建一批消息，循环复用，避免热路径分配
                    let messages = Arc::new(prebuild_messages(message_count_per_round, 30_000));

                    // 计数与启动同步
                    let start_barrier = Arc::new(Barrier::new(2));
                    let sent = Arc::new(AtomicU64::new(0));
                    let received = Arc::new(AtomicU64::new(0));

                    // 消费者线程：持续 wait + drain，直到收满 total_to_send
                    let cns = consumer.clone();
                    let start_c = start_barrier.clone();
                    let recv_cnt = received.clone();
                    let total_target = total_to_send as u64;
                    let h_cons = std::thread::spawn(move || {
                        start_c.wait();
                        while recv_cnt.load(Ordering::Acquire) < total_target {
                            // 避免空转，等待最多1ms
                            let _ = cns.wait_for_message(Some(Duration::from_millis(1)));
                            // 尽可能多地读取
                            while let Ok(Some(_)) = cns.try_read_next_message() {
                                recv_cnt.fetch_add(1, Ordering::Release);
                                if recv_cnt.load(Ordering::Acquire) >= total_target {
                                    break;
                                }
                            }
                        }
                    });

                    // 生产者线程：严格 SPSC，只写不读，直到发满 total_to_send
                    let p = producer.clone();
                    let start_p = start_barrier.clone();
                    let sent_cnt = sent.clone();
                    let msgs = messages.clone();
                    let h_prod = std::thread::spawn(move || {
                        start_p.wait();
                        let mut idx = 0usize;
                        while sent_cnt.load(Ordering::Acquire) < total_target {
                            let m = &msgs[idx];
                            // 若满则忙等等待消费者清空（不可读！）
                            while !p.try_write_message(m).unwrap_or(false) {
                                std::hint::spin_loop();
                            }
                            sent_cnt.fetch_add(1, Ordering::Release);
                            idx += 1;
                            if idx == msgs.len() {
                                idx = 0;
                            }
                        }
                    });

                    // 启动后才计时
                    let t0 = Instant::now();
                    let _ = h_prod.join();
                    let _ = h_cons.join();
                    let elapsed = t0.elapsed();

                    // 基本校验（不打印）
                    debug_assert_eq!(sent.load(Ordering::Acquire), total_to_send as u64);
                    debug_assert_eq!(received.load(Ordering::Acquire), total_to_send as u64);

                    // 清理
                    drop(producer);
                    drop(consumer);
                    let _ = std::fs::remove_file(&test_path);

                    elapsed
                });
            },
        );
    }
    group.finish();
}

// 5) 命令往返
fn bench_command_latency(c: &mut Criterion) {
    let test_path = mk_path("bench_cmd_latency");
    let _ = std::fs::remove_file(&test_path);

    let sender = SharedRingBuffer::create(&test_path, Some(1024), Some(1000)).unwrap();
    thread::sleep(Duration::from_millis(5));
    let receiver = SharedRingBuffer::open(&test_path, Some(1000)).unwrap();

    c.bench_function("command_round_trip", |b| {
        b.iter(|| {
            let command = black_box(SharedCommand::view_tag(1 << 3, 0));
            if sender.send_command(command).unwrap_or(false) {
                let _ = receiver.wait_for_command(Some(Duration::from_millis(5)));
                let _ = receiver.receive_command();
            }
        })
    });

    drop(sender);
    drop(receiver);
    let _ = std::fs::remove_file(&test_path);
}

// 6) 内存布局压力
fn bench_memory_layout_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_layout");

    for &buffer_size in &[16usize, 64, 256, 1024, 4096] {
        if (buffer_size as u32).is_power_of_two() {
            group.bench_with_input(
                BenchmarkId::new("buffer_size", buffer_size),
                &buffer_size,
                |b, &size| {
                    let test_path = mk_path(&format!("bench_layout_{}", size));
                    let _ = std::fs::remove_file(&test_path);

                    let buffer =
                        SharedRingBuffer::create(&test_path, Some(size), Some(1000)).unwrap();
                    let prefill_msgs = prebuild_messages((size * 3) / 4, 40_000);
                    let alternation_msgs = prebuild_messages(100, 41_000);

                    b.iter(|| {
                        drain_all(&buffer);
                        // 预填充至 75%
                        for m in &prefill_msgs {
                            if !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                                break;
                            }
                        }

                        // 交替读写
                        for m in &alternation_msgs {
                            let _ = buffer.try_read_next_message();
                            if !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                                let _ = buffer.try_read_next_message();
                                let _ = buffer.try_write_message(black_box(m));
                            }
                        }
                    });

                    drop(buffer);
                    let _ = std::fs::remove_file(&test_path);
                },
            );
        }
    }
    group.finish();
}

// 7) 突发性能
fn bench_burst_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("burst_performance");

    for &burst_size in &[10usize, 50, 100, 500] {
        group.bench_with_input(
            BenchmarkId::new("burst_write_read", burst_size),
            &burst_size,
            |b, &size| {
                let test_path = mk_path(&format!("bench_burst_{}", size));
                let _ = std::fs::remove_file(&test_path);

                let buffer = SharedRingBuffer::create(&test_path, Some(2048), Some(0)).unwrap();
                let burst_msgs = prebuild_messages(size, 50_000);

                b.iter(|| {
                    drain_all(&buffer);
                    // 突发写入
                    for m in &burst_msgs {
                        while !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                            let _ = buffer.try_read_next_message();
                        }
                    }
                    // 突发读取
                    let mut read_count = 0usize;
                    while let Ok(Some(_)) = buffer.try_read_next_message() {
                        read_count += 1;
                        if read_count >= size {
                            break;
                        }
                    }
                    black_box(read_count);
                });

                drop(buffer);
                let _ = std::fs::remove_file(&test_path);
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_single_threaded_write,
    bench_single_threaded_read,
    bench_throughput_varying_sizes,
    bench_producer_consumer,
    bench_command_latency,
    bench_memory_layout_efficiency,
    bench_burst_performance,
);
criterion_main!(benches);
