// benches/ring_buffer_bench.rs
use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
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

                // 使用 iter_custom：每个样本只创建一次共享段与线程，执行 iters 轮数据传输
                b.iter_custom(|iters| {
                    // 构建共享段
                    let producer = Arc::new(
                        SharedRingBuffer::create(&test_path, Some(1024), Some(spins)).unwrap(),
                    );
                    thread::sleep(Duration::from_millis(5));
                    let consumer =
                        Arc::new(SharedRingBuffer::open(&test_path, Some(spins)).unwrap());

                    let barrier = Arc::new(Barrier::new(2));
                    let rounds = Arc::new(AtomicU64::new(0));
                    let rounds_target = iters; // 每个样本进行 iters 轮

                    // 每轮的消息数
                    let message_count = 1000usize;
                    let messages = Arc::new(prebuild_messages(message_count, 30_000));

                    let p = producer.clone();
                    let cns = consumer.clone();
                    let b1 = barrier.clone();
                    let b2 = barrier.clone();
                    let msgs_for_consumer = messages.clone();
                    let msgs_for_producer = messages.clone();
                    let rounds_c = rounds.clone();
                    let rounds_p = rounds.clone();

                    let start = Instant::now();

                    // 消费者线程（持续工作到达 rounds_target 轮）
                    let h_cons = thread::spawn(move || {
                        b1.wait();
                        let mut received_in_round = 0usize;
                        let mut current_round = 0u64;

                        loop {
                            if current_round >= rounds_target {
                                break;
                            }
                            // 等待/拉取消息
                            let _ = cns.wait_for_message(Some(Duration::from_millis(2)));
                            while let Ok(Some(_)) = cns.try_read_next_message() {
                                received_in_round += 1;
                                if received_in_round >= msgs_for_consumer.len() {
                                    // 一轮结束
                                    current_round += 1;
                                    received_in_round = 0;
                                }
                            }
                        }
                    });

                    // 生产者线程
                    let h_prod = thread::spawn(move || {
                        b2.wait();
                        let mut current_round = 0u64;
                        while current_round < rounds_target {
                            // 每轮先清空
                            drain_all(&p);
                            for m in msgs_for_producer.iter() {
                                while !p.try_write_message(m).unwrap_or(false) {
                                    let _ = p.try_read_next_message();
                                }
                            }
                            current_round += 1;
                            rounds_p.store(current_round, Ordering::Release);
                        }
                    });

                    let _ = h_prod.join();
                    let _ = h_cons.join();

                    let elapsed = start.elapsed();

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
