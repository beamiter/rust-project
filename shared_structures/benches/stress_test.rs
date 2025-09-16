// benches/stress_test.rs
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::hint::black_box;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Arc, Barrier,
};
use std::thread;
use std::time::{Duration, Instant};

fn mk_path(name: &str) -> String {
    format!("/tmp/{}_{}", name, std::process::id())
}

fn drain_all(buffer: &SharedRingBuffer) {
    // 顺序读取清空，避免 latest 跳跃带来的歧义
    while let Ok(Some(_)) = buffer.try_read_next_message() {}
}

fn create_base_message(id: i32) -> SharedMessage {
    // 固定 client_name 和 ltsymbol，避免循环中分配与格式化
    let mut message = SharedMessage::default();
    let mi = message.get_monitor_info_mut();
    mi.monitor_num = id;
    mi.set_client_name("test_client");
    mi.set_ltsymbol("[]=");
    message
}

fn prebuild_messages(count: usize, base_id: i32) -> Vec<SharedMessage> {
    let mut v = Vec::with_capacity(count);
    for i in 0..count {
        v.push(create_base_message(base_id + i as i32));
    }
    v
}

// 一、单写多读负载的高频更新（实质是单写单读，消费者驻留线程）
// 目标：测量“执行固定条数写入”的耗时，消费者常驻持续读取，避免缓冲区顶满。
// 使用 iter_custom：一次样本内启动消费者线程，只计“固定条数写入”的总时间。
fn bench_high_frequency_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_frequency");
    group.sample_size(12);

    // 配置不同的自适应自旋次数，覆盖等待路径
    for &spins in [0u32, 1000, 5000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("updates", spins),
            &spins,
            |b, &spin_count| {
                b.iter_custom(|iters| {
                    let test_path = mk_path(&format!("stress_high_freq_{}", spin_count));
                    let _ = std::fs::remove_file(&test_path);

                    let buffer = Arc::new(
                        SharedRingBuffer::create_aux(&test_path, Some(4096), Some(spin_count))
                            .unwrap(),
                    );
                    // 常驻消费者线程：不断拉取，避免写端顶满
                    let stop = Arc::new(AtomicBool::new(false));
                    let b_cons = buffer.clone();
                    let stop_cons = stop.clone();
                    let consumer = thread::spawn(move || {
                        while !stop_cons.load(Ordering::Relaxed) {
                            // 优先等待，避免忙等占用 CPU
                            let _ = b_cons.wait_for_message(Some(Duration::from_millis(1)));
                            while let Ok(Some(_)) = b_cons.try_read_next_message() {}
                        }
                        // 退出前清空残留
                        while let Ok(Some(_)) = b_cons.try_read_next_message() {}
                    });

                    // 预构建目标写入数据；每次“迭代”写固定 batch 的消息
                    let batch_writes: usize = 2000;
                    let messages = prebuild_messages(batch_writes, 1000);

                    // 执行 iters 轮，每轮写 batch_writes 条，累计耗时
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        // 开始计时
                        let t0 = Instant::now();
                        for m in &messages {
                            while !buffer.try_write_message(black_box(m)).unwrap_or(false) {
                                // 只写不读（严格 SPSC），等待消费者清空
                                std::hint::spin_loop();
                            }
                        }
                        total += t0.elapsed();

                        // 快速 drain（消费者通常已经清完，这里只是兜底）
                        // 注：不要在写者侧调用 try_read_next_message，保持 SPSC 语义纯净
                    }

                    // 停止消费者
                    stop.store(true, Ordering::Relaxed);
                    let _ = consumer.join();

                    drop(buffer);
                    let _ = std::fs::remove_file(&test_path);

                    total
                });
            },
        );
    }
    group.finish();
}

// 二、多生产者压力（通过 MPSC 聚合到单写者，再到 SPSC 环，再单消费者）
// 目标：每轮固定条目总数（producers * per_producer），测量端到端完成耗时。
fn bench_concurrent_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_stress");
    group.sample_size(10);

    for &num_producers in [1usize, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("producers", num_producers),
            &num_producers,
            |b, &producer_count| {
                b.iter_custom(|iters| {
                    let test_path = mk_path(&format!("stress_concurrent_{}", producer_count));
                    let _ = std::fs::remove_file(&test_path);

                    // SPSC 环：单写者 + 单读者
                    let writer_rb = Arc::new(
                        SharedRingBuffer::create_aux(&test_path, Some(4096), Some(5000)).unwrap(),
                    );
                    thread::sleep(Duration::from_millis(5));
                    let reader_rb =
                        Arc::new(SharedRingBuffer::open_aux(&test_path, Some(5000)).unwrap());

                    // MPSC 管道：多生产者 -> 单聚合写者
                    let (tx, rx) = mpsc::channel::<u32>();

                    // 常驻聚合写者线程：从 rx 取 -> 写入环
                    let wr = writer_rb.clone();
                    let aggregator_running = Arc::new(AtomicBool::new(true));
                    let aggregator_running_c = aggregator_running.clone();
                    let writer = thread::spawn(move || {
                        let mut msg = create_base_message(0);
                        while aggregator_running_c.load(Ordering::Acquire) {
                            match rx.recv_timeout(Duration::from_millis(1)) {
                                Ok(v) => {
                                    msg.get_monitor_info_mut().monitor_num = v as i32;
                                    // 满则忙等等待消费者清空
                                    while !wr.try_write_message(&msg).unwrap_or(false) {
                                        std::hint::spin_loop();
                                    }
                                }
                                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            }
                        }
                        // 排空可能残留
                        while let Ok(v) = rx.try_recv() {
                            msg.get_monitor_info_mut().monitor_num = v as i32;
                            while !wr.try_write_message(&msg).unwrap_or(false) {
                                std::hint::spin_loop();
                            }
                        }
                    });

                    // 常驻消费者线程：从环读
                    let rd = reader_rb.clone();
                    let consumer_running = Arc::new(AtomicBool::new(true));
                    let consumer_running_c = consumer_running.clone();
                    let consumed_total = Arc::new(AtomicUsize::new(0));
                    let consumed_total_c = consumed_total.clone();
                    let consumer = thread::spawn(move || {
                        while consumer_running_c.load(Ordering::Acquire) {
                            let _ = rd.wait_for_message(Some(Duration::from_millis(1)));
                            while let Ok(Some(_)) = rd.try_read_next_message() {
                                consumed_total_c.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        // 兜底排空
                        while let Ok(Some(_)) = rd.try_read_next_message() {
                            consumed_total_c.fetch_add(1, Ordering::Relaxed);
                        }
                    });

                    // 每轮样本执行：创建临时生产者们，固定发送条目数
                    let per_producer: usize = 2000;
                    let mut total = Duration::ZERO;

                    for _ in 0..iters {
                        let barrier = Arc::new(Barrier::new(producer_count));
                        let mut handles = Vec::with_capacity(producer_count);

                        // 计数目标：用于等待 round 完成
                        let start_consumed = consumed_total.load(Ordering::Acquire);
                        let target_total = start_consumed + producer_count * per_producer;

                        for p in 0..producer_count {
                            let tx_i = tx.clone();
                            let b = barrier.clone();
                            let h = thread::spawn(move || {
                                // 每个生产者发送固定条数，避免“时间驱动”测量的噪声
                                b.wait();
                                for i in 0..per_producer {
                                    let id = ((p as u32) << 24) | (i as u32);
                                    // mpsc send 是阻塞内存队列，失败仅在断开
                                    if tx_i.send(id).is_err() {
                                        break;
                                    }
                                }
                            });
                            handles.push(h);
                        }

                        // 计时开始：从全部生产者同步起跑
                        let t0 = Instant::now();

                        // 等待所有生产者发送完成
                        for h in handles {
                            let _ = h.join();
                        }

                        // 等待消费者完成本轮消费
                        while consumed_total.load(Ordering::Acquire) < target_total {
                            thread::yield_now();
                        }

                        total += t0.elapsed();
                    }

                    // 关闭常驻线程
                    aggregator_running.store(false, Ordering::Release);
                    consumer_running.store(false, Ordering::Release);
                    let _ = writer.join();
                    let _ = consumer.join();

                    drop(writer_rb);
                    drop(reader_rb);
                    let _ = std::fs::remove_file(&test_path);

                    total
                });
            },
        );
    }
    group.finish();
}

// 三、内存压力：固定工作量（每轮写入 size*10 条，读出全部），测量完成时间
fn bench_memory_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pressure");
    group.sample_size(10);

    for &buffer_size in [64usize, 256, 1024, 4096].iter() {
        if (buffer_size as u32).is_power_of_two() {
            group.bench_with_input(
                BenchmarkId::new("buffer_size", buffer_size),
                &buffer_size,
                |b, &size| {
                    b.iter_custom(|iters| {
                        let test_path = mk_path(&format!("stress_memory_{}", size));
                        let _ = std::fs::remove_file(&test_path);

                        let buffer = Arc::new(
                            SharedRingBuffer::create_aux(&test_path, Some(size), Some(2000))
                                .unwrap(),
                        );
                        let reader = buffer.clone();

                        // 常驻读者线程：持续拉取
                        let running = Arc::new(AtomicBool::new(true));
                        let running_c = running.clone();
                        let read_counter = Arc::new(AtomicUsize::new(0));
                        let read_counter_c = read_counter.clone();

                        let consumer = thread::spawn(move || {
                            while running_c.load(Ordering::Acquire) {
                                let _ = reader.wait_for_message(Some(Duration::from_millis(1)));
                                while let Ok(Some(_)) = reader.try_read_next_message() {
                                    read_counter_c.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            while let Ok(Some(_)) = reader.try_read_next_message() {
                                read_counter_c.fetch_add(1, Ordering::Relaxed);
                            }
                        });

                        let mut total = Duration::ZERO;
                        let writes_per_round = size * 10;
                        let mut msg = create_base_message(0);

                        for round in 0..iters {
                            let start_read = read_counter.load(Ordering::Acquire);
                            let target = start_read + writes_per_round;

                            let t0 = Instant::now();
                            for i in 0..writes_per_round {
                                msg.get_monitor_info_mut().monitor_num =
                                    (round as usize * writes_per_round + i) as i32;
                                while !buffer.try_write_message(&msg).unwrap_or(false) {
                                    std::hint::spin_loop();
                                }
                            }
                            // 等待读者读完本轮
                            while read_counter.load(Ordering::Acquire) < target {
                                thread::yield_now();
                            }
                            total += t0.elapsed();
                        }

                        running.store(false, Ordering::Release);
                        let _ = consumer.join();

                        drop(buffer);
                        let _ = std::fs::remove_file(&test_path);

                        total
                    });
                },
            );
        }
    }
    group.finish();
}

// 四、命令压力：固定条目数往返，测量完成时间
fn bench_command_stress(c: &mut Criterion) {
    c.bench_function("command_stress", |b| {
        b.iter_custom(|iters| {
            let test_path = mk_path("stress_commands");
            let _ = std::fs::remove_file(&test_path);

            let sender =
                Arc::new(SharedRingBuffer::create_aux(&test_path, Some(1024), Some(3000)).unwrap());
            thread::sleep(Duration::from_millis(5));
            let receiver = Arc::new(SharedRingBuffer::open_aux(&test_path, Some(3000)).unwrap());

            let recv_counter = Arc::new(AtomicUsize::new(0));
            let running = Arc::new(AtomicBool::new(true));

            // 常驻接收线程：持续 wait + drain
            let r = receiver.clone();
            let recv_c = recv_counter.clone();
            let running_c = running.clone();
            let consumer = thread::spawn(move || {
                while running_c.load(Ordering::Acquire) {
                    if r.wait_for_command(Some(Duration::from_millis(1)))
                        .unwrap_or(false)
                    {
                        while let Some(_) = r.receive_command() {
                            recv_c.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        while let Some(_) = r.receive_command() {
                            recv_c.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                while let Some(_) = r.receive_command() {
                    recv_c.fetch_add(1, Ordering::Relaxed);
                }
            });

            let mut total = Duration::ZERO;
            let cmds_per_round = 2000;

            for _ in 0..iters {
                let start_recv = recv_counter.load(Ordering::Acquire);
                let target = start_recv + cmds_per_round;

                let t0 = Instant::now();
                for i in 0..cmds_per_round {
                    let cmd = SharedCommand::view_tag(1 << (i % 9), (i % 2) as i32);
                    while !sender.send_command(black_box(cmd)).unwrap_or(false) {
                        std::hint::spin_loop();
                    }
                }

                while recv_counter.load(Ordering::Acquire) < target {
                    thread::yield_now();
                }
                total += t0.elapsed();
            }

            running.store(false, Ordering::Release);
            let _ = consumer.join();

            drop(sender);
            drop(receiver);
            let _ = std::fs::remove_file(&test_path);

            total
        });
    });
}

// 五、长时间稳定性：固定工作量，测量多轮执行总时间（主要用于回归与稳定性观察）
fn bench_long_running_stability(c: &mut Criterion) {
    c.bench_function("long_running_stability", |b| {
        b.iter_batched(
            || {
                let test_path = mk_path("stress_long_running");
                let _ = std::fs::remove_file(&test_path);
                let buffer = Arc::new(
                    SharedRingBuffer::create_aux(&test_path, Some(1024), Some(4000)).unwrap(),
                );
                (test_path, buffer)
            },
            |(test_path, buffer)| {
                let total_cycles = 10usize;
                let messages_per_cycle = 100usize;
                let mut msg = create_base_message(0);

                // 写入阶段 + 读取阶段成对进行，固定工作量
                for cycle in 0..total_cycles {
                    // 写入固定条数
                    for i in 0..messages_per_cycle {
                        msg.get_monitor_info_mut().monitor_num =
                            (cycle * messages_per_cycle + i) as i32;
                        while !buffer.try_write_message(&msg).unwrap_or(false) {
                            std::hint::spin_loop();
                        }
                    }

                    // 读取固定条数
                    let mut read_in_cycle = 0;
                    while let Ok(Some(_)) = buffer.try_read_next_message() {
                        read_in_cycle += 1;
                        if read_in_cycle >= messages_per_cycle {
                            break;
                        }
                    }
                }

                // 最终清理
                drain_all(&buffer);

                drop(buffer);
                let _ = std::fs::remove_file(&test_path);
            },
            BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    stress_tests,
    bench_high_frequency_updates,
    bench_concurrent_stress,
    bench_memory_pressure,
    bench_command_stress,
    bench_long_running_stability
);
criterion_main!(stress_tests);
