// benches/ring_buffer_bench.rs
use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

// 请根据实际情况调整模块路径
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};

// 统一的辅助函数与工具
fn mk_path(name: &str) -> String {
    format!("/tmp/{}_{}", name, std::process::id())
}

fn drain_all(buffer: &SharedRingBuffer) {
    // 使用“顺序读取”避免 latest 的跳跃行为带来的歧义，且可完全清空
    while let Ok(Some(_)) = buffer.try_read_next_message() {}
}

fn prebuild_messages(count: usize, base_id: i32) -> Vec<SharedMessage> {
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        v.push(create_test_message(base_id + i as i32));
    }
    v
}

// 辅助函数：构建一个测试消息（仅构建一次，避免循环内格式化分配）
fn create_test_message(id: i32) -> SharedMessage {
    let mut message = SharedMessage::default();
    message.get_monitor_info_mut().monitor_num = id;
    message
        .get_monitor_info_mut()
        .set_client_name(&format!("test_client_{}", id));
    message.get_monitor_info_mut().set_ltsymbol("[]=");
    message
}

// 1) 单线程写入基准：预先构造 100 条消息，每次迭代清空 -> 写入
fn bench_single_threaded_write(c: &mut Criterion) {
    let test_path = mk_path("bench_single_write");
    let _ = std::fs::remove_file(&test_path);

    let buffer = SharedRingBuffer::create(&test_path, Some(1024), Some(0)).unwrap();
    let messages = prebuild_messages(100, 0);

    c.bench_function("single_threaded_write", |b| {
        b.iter(|| {
            drain_all(&buffer);
            for m in &messages {
                // 持续重试，必要时读取一条释放空间
                while !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                    let _ = buffer.try_read_next_message();
                }
            }
            black_box(buffer.available_messages());
        })
    });

    // 清理
    drop(buffer);
    let _ = std::fs::remove_file(&test_path);
}

// 2) 单线程读取基准：使用 iter_batched，将“填充阶段”和“读取阶段”分离
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
                // 返回引用或轻量数据作为测量阶段的输入
                ()
            },
            |_| {
                // 纯读取阶段
                while let Ok(Some(_)) = buffer.try_read_next_message() {}
            },
            BatchSize::SmallInput,
        )
    });

    drop(buffer);
    let _ = std::fs::remove_file(&test_path);
}

// 3) 吞吐量（不同消息数）基准：预构建消息向量，迭代内只读写
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

// 4) 生产者-消费者：更稳健的实现，去掉热路径中的 eprintln，预先构建消息
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
                    // 每次样本测量使用 iter_custom，重复 iters 轮，每轮发送固定消息数
                    // 注意：线程创建仍在样本热路径中，如需更纯粹测量可改为“持久线程+Barrier 多轮”方案。
                    let mut total = Duration::ZERO;
                    let message_count = 1000usize;
                    let messages = prebuild_messages(message_count, 30_000);

                    for _ in 0..iters {
                        // 创建共享缓冲区（生产者）
                        let producer = Arc::new(
                            SharedRingBuffer::create(&test_path, Some(1024), Some(spins)).unwrap(),
                        );

                        // 稍等，确保创建完成
                        thread::sleep(Duration::from_millis(5));

                        // 打开消费者
                        let consumer =
                            Arc::new(SharedRingBuffer::open(&test_path, Some(spins)).unwrap());

                        let barrier = Arc::new(Barrier::new(2));
                        let sent = Arc::new(AtomicU64::new(0));
                        let recv = Arc::new(AtomicU64::new(0));

                        let b1 = barrier.clone();
                        let b2 = barrier.clone();
                        let p = producer.clone();
                        let cns = consumer.clone();
                        let sent_c = sent.clone();
                        let recv_c = recv.clone();
                        let msgs_for_consumer = messages.clone();

                        let t0 = Instant::now();

                        // 消费者
                        let h_cons = thread::spawn(move || {
                            b1.wait();
                            let mut value = msgs_for_consumer.len();
                            while recv_c.load(Ordering::Acquire) < value as u64 {
                                // 等待/或获取可用消息
                                let _ = cns.wait_for_message(Some(Duration::from_millis(1)));
                                while let Ok(Some(_)) = cns.try_read_next_message() {
                                    recv_c.fetch_add(1, Ordering::Release);
                                }
                            }
                        });

                        // 生产者
                        let msgs_for_product = messages.clone();
                        let h_prod = thread::spawn(move || {
                            b2.wait();
                            for m in &msgs_for_product {
                                while !p.try_write_message(m).unwrap_or(false) {
                                    // 让出
                                    let _ = p.try_read_next_message();
                                }
                                sent_c.fetch_add(1, Ordering::Release);
                            }
                        });

                        let _ = h_prod.join();
                        let _ = h_cons.join();

                        total += t0.elapsed();

                        // 清理
                        drop(producer);
                        drop(consumer);
                        let _ = std::fs::remove_file(&test_path);

                        // 基本校验（不在热路径打印）
                        debug_assert_eq!(sent.load(Ordering::Acquire), message_count as u64);
                        debug_assert_eq!(recv.load(Ordering::Acquire), message_count as u64);
                    }
                    total
                });
            },
        );
    }
    group.finish();
}

// 5) 命令往返时延：保持简单，移除热路径中的 I/O
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
                // 等待并接收
                let _ = receiver.wait_for_command(Some(Duration::from_millis(5)));
                let _ = receiver.receive_command();
            }
        })
    });

    drop(sender);
    drop(receiver);
    let _ = std::fs::remove_file(&test_path);
}

// 6) 布局与容量压力测试：预构建消息，减少打印
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
                        // 预填充至 75%
                        drain_all(&buffer);
                        for m in &prefill_msgs {
                            if !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                                break;
                            }
                        }

                        // 交替读写
                        for m in &alternation_msgs {
                            // 尝试读一条
                            let _ = buffer.try_read_next_message();

                            // 尝试写入（失败则先读一条再重试一次）
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

// 7) 突发性能：预构建突发消息，简化逻辑，减少 I/O
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
                    // 突发写入
                    drain_all(&buffer);
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

// 8) 自适应轮询效果：保持结构，但减少迭代中分配、构造
fn bench_adaptive_polling_effectiveness(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_polling");

    for &spins in &[0u32, 100, 1000, 5000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("spin_effectiveness", spins),
            &spins,
            |b, &spins| {
                let test_path = mk_path(&format!("bench_adaptive_{}", spins));
                let _ = std::fs::remove_file(&test_path);

                let writer =
                    Arc::new(SharedRingBuffer::create(&test_path, Some(512), Some(spins)).unwrap());
                thread::sleep(Duration::from_millis(5));
                let reader = Arc::new(SharedRingBuffer::open(&test_path, Some(spins)).unwrap());

                let msg = create_test_message(42);

                b.iter(|| {
                    let w = writer.clone();
                    let r = reader.clone();
                    let barrier = Arc::new(Barrier::new(2));
                    let b1 = barrier.clone();
                    let b2 = barrier.clone();

                    // 写入线程
                    let hw = thread::spawn(move || {
                        b1.wait();
                        let _ = w.try_write_message(black_box(&msg));
                    });

                    // 读取线程
                    let hr = thread::spawn(move || {
                        b2.wait();
                        let _ = r.wait_for_message(Some(Duration::from_millis(5)));
                        let _ = r.try_read_next_message();
                    });

                    let _ = hw.join();
                    let _ = hr.join();
                });

                drop(writer);
                drop(reader);
                let _ = std::fs::remove_file(&test_path);
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    // bench_single_threaded_write,
    // bench_single_threaded_read,
    // bench_throughput_varying_sizes,
    bench_producer_consumer,
    // bench_command_latency,
    // bench_memory_layout_efficiency,
    // bench_burst_performance,
    // bench_adaptive_polling_effectiveness
);
criterion_main!(benches);
