// benches/stress_test.rs
use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
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

fn bench_high_frequency_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_frequency");
    group.sample_size(20);

    for &spins in [0u32, 1000, 5000, 10_000].iter() {
        group.bench_with_input(
            BenchmarkId::new("updates", spins),
            &spins,
            |b, &spin_count| {
                b.iter_batched(
                    || {
                        let test_path = mk_path(&format!("stress_high_freq_{}", spin_count));
                        let _ = std::fs::remove_file(&test_path);

                        let buffer = Arc::new(
                            SharedRingBuffer::create(&test_path, Some(2048), Some(spin_count))
                                .unwrap(),
                        );
                        // 统一停止标志与计数器
                        let stop = Arc::new(AtomicBool::new(false));
                        let updates = Arc::new(AtomicU64::new(0));
                        let errors = Arc::new(AtomicU64::new(0));
                        (test_path, buffer, stop, updates, errors)
                    },
                    |(test_path, buffer, stop, updates, errors)| {
                        // 高频生产者：仅修改 monitor_num，复用同一个 message 实例以减少复制
                        let b2 = buffer.clone();
                        let stop2 = stop.clone();
                        let upd2 = updates.clone();
                        let err2 = errors.clone();

                        let producer = thread::spawn(move || {
                            let mut msg = create_base_message(0);
                            let mut counter: u32 = 0;
                            let mut retry = 0u32;

                            while !stop2.load(Ordering::Relaxed) {
                                msg.get_monitor_info_mut().monitor_num = counter as i32;
                                match b2.try_write_message(&msg) {
                                    Ok(true) => {
                                        upd2.fetch_add(1, Ordering::Relaxed);
                                        counter = counter.wrapping_add(1);
                                        retry = 0;
                                    }
                                    Ok(false) => {
                                        // 满：短暂退让
                                        retry += 1;
                                        if retry % 1024 == 0 {
                                            std::hint::spin_loop();
                                        }
                                    }
                                    Err(_) => {
                                        err2.fetch_add(1, Ordering::Relaxed);
                                        break;
                                    }
                                }
                            }
                        });

                        // 运行固定时长
                        thread::sleep(Duration::from_millis(100));
                        stop.store(true, Ordering::Relaxed);
                        let _ = producer.join();

                        // 读取剩余
                        drain_all(&buffer);

                        // 简要校验（不打印，减少噪声）
                        let _final_updates = updates.load(Ordering::Relaxed);
                        let final_errors = errors.load(Ordering::Relaxed);
                        debug_assert_eq!(final_errors, 0);

                        // 清理
                        drop(buffer);
                        let _ = std::fs::remove_file(&test_path);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

// 注意：SharedRingBuffer 是 SPSC（单生产者单消费者）
// 这里实现“多生产者压力”时，不允许多个线程直接写入同一个环。
// 方案：多生产者 -> MPSC 通道 -> 单聚合写者 -> SPSC 环 -> 单消费者
fn bench_concurrent_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_stress");
    group.sample_size(10);

    for &num_producers in [1usize, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("producers", num_producers),
            &num_producers,
            |b, &producer_count| {
                b.iter_batched(
                    || {
                        let test_path = mk_path(&format!("stress_concurrent_{}", producer_count));
                        let _ = std::fs::remove_file(&test_path);

                        // SPSC 环：单写者 + 单读者
                        let writer_rb = Arc::new(
                            SharedRingBuffer::create(&test_path, Some(4096), Some(5000)).unwrap(),
                        );
                        thread::sleep(Duration::from_millis(5));
                        let reader_rb =
                            Arc::new(SharedRingBuffer::open(&test_path, Some(5000)).unwrap());

                        // 多生产者 -> MPSC
                        let (tx, rx) = mpsc::channel::<u32>();
                        let barrier = Arc::new(Barrier::new(producer_count + 2)); // + writer + consumer
                        let test_duration = Duration::from_millis(100);

                        (
                            test_path,
                            writer_rb,
                            reader_rb,
                            tx,
                            rx,
                            barrier,
                            test_duration,
                            producer_count,
                        )
                    },
                    |(
                        test_path,
                        writer_rb,
                        reader_rb,
                        tx,
                        rx,
                        barrier,
                        test_duration,
                        producer_count,
                    )| {
                        let total_produced = Arc::new(AtomicU64::new(0));
                        let total_consumed = Arc::new(AtomicU64::new(0));

                        // 启动生产者们（只发编号）
                        let mut handles = Vec::with_capacity(producer_count);
                        for p in 0..producer_count {
                            let tx_i = tx.clone();
                            let barrier_i = barrier.clone();
                            let produced_i = total_produced.clone();
                            let h = thread::spawn(move || {
                                barrier_i.wait();
                                let start = Instant::now();
                                let mut local: u32 = 0;
                                while start.elapsed() < test_duration {
                                    // 编号混入线程 id 以便唯一性（非必要，但便于调试）
                                    let id = ((p as u32) << 24) | local;
                                    if tx_i.send(id).is_ok() {
                                        local = local.wrapping_add(1);
                                    }
                                }
                                produced_i.fetch_add(local as u64, Ordering::Relaxed);
                            });
                            handles.push(h);
                        }

                        // 单一聚合写者：从 rx 取数据，写入环（保持 SPSC）
                        let barrier_w = barrier.clone();
                        let writer_rb_w = writer_rb.clone();
                        let writer = thread::spawn(move || {
                            barrier_w.wait();
                            let start = Instant::now();
                            let mut msg = create_base_message(0);
                            while start.elapsed() < test_duration || !rx.try_recv().is_err() {
                                match rx.try_recv() {
                                    Ok(v) => {
                                        msg.get_monitor_info_mut().monitor_num = v as i32;
                                        // 满则尝试清空一条（非必要，但尽量不阻塞）
                                        while !writer_rb_w.try_write_message(&msg).unwrap_or(false)
                                        {
                                            let _ = writer_rb_w.try_read_next_message();
                                        }
                                    }
                                    Err(mpsc::TryRecvError::Empty) => {
                                        std::hint::spin_loop();
                                    }
                                    Err(mpsc::TryRecvError::Disconnected) => break,
                                }
                            }
                        });

                        // 单消费者：从环读数据
                        let barrier_c = barrier.clone();
                        let reader_rb_c = reader_rb.clone();
                        let consumed_c = total_consumed.clone();
                        let consumer = thread::spawn(move || {
                            barrier_c.wait();
                            let start = Instant::now();
                            while start.elapsed() < test_duration {
                                let _ =
                                    reader_rb_c.wait_for_message(Some(Duration::from_micros(200)));
                                while let Ok(Some(_)) = reader_rb_c.try_read_next_message() {
                                    consumed_c.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            // 清空剩余
                            while let Ok(Some(_)) = reader_rb_c.try_read_next_message() {
                                consumed_c.fetch_add(1, Ordering::Relaxed);
                            }
                        });

                        // 等待线程结束
                        for h in handles {
                            let _ = h.join();
                        }
                        let _ = writer.join();
                        let _ = consumer.join();

                        // 校验（不打印）
                        let produced = total_produced.load(Ordering::Relaxed);
                        let consumed = total_consumed.load(Ordering::Relaxed);
                        debug_assert!(consumed <= produced);

                        // 清理
                        drop(writer_rb);
                        drop(reader_rb);
                        let _ = std::fs::remove_file(&test_path);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_memory_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pressure");
    group.sample_size(10);

    for &buffer_size in [64usize, 256, 1024, 4096].iter() {
        if (buffer_size as u32).is_power_of_two() {
            group.bench_with_input(
                BenchmarkId::new("buffer_size", buffer_size),
                &buffer_size,
                |b, &size| {
                    b.iter_batched(
                        || {
                            let test_path = mk_path(&format!("stress_memory_{}", size));
                            let _ = std::fs::remove_file(&test_path);

                            let buffer = Arc::new(
                                SharedRingBuffer::create(&test_path, Some(size), Some(2000))
                                    .unwrap(),
                            );

                            (test_path, buffer, size)
                        },
                        |(test_path, buffer, size)| {
                            let iterations = size * 10;
                            let write_count = Arc::new(AtomicUsize::new(0));
                            let read_count = Arc::new(AtomicUsize::new(0));
                            let barrier = Arc::new(Barrier::new(2));

                            // 写线程：复用 message，仅改 monitor_num
                            let b_w = buffer.clone();
                            let wc = write_count.clone();
                            let bar_w = barrier.clone();
                            let writer = thread::spawn(move || {
                                bar_w.wait();
                                let mut msg = create_base_message(0);
                                for i in 0..iterations {
                                    msg.get_monitor_info_mut().monitor_num = i as i32;
                                    let mut retry = 0;
                                    while !b_w.try_write_message(&msg).unwrap_or(false) {
                                        retry += 1;
                                        if retry > 1000 {
                                            break;
                                        }
                                        std::hint::spin_loop();
                                    }
                                    wc.fetch_add(1, Ordering::Relaxed);
                                }
                            });

                            // 读线程：顺序读取
                            let b_r = buffer.clone();
                            let rc = read_count.clone();
                            let bar_r = barrier.clone();
                            let reader = thread::spawn(move || {
                                bar_r.wait();
                                let start = Instant::now();
                                while start.elapsed() < Duration::from_millis(200) {
                                    match b_r.try_read_next_message() {
                                        Ok(Some(_)) => {
                                            rc.fetch_add(1, Ordering::Relaxed);
                                        }
                                        Ok(None) => std::hint::spin_loop(),
                                        Err(_) => break,
                                    }
                                }
                                // 清空剩余
                                while let Ok(Some(_)) = b_r.try_read_next_message() {
                                    rc.fetch_add(1, Ordering::Relaxed);
                                }
                            });

                            let _ = writer.join();
                            let _ = reader.join();

                            let final_w = write_count.load(Ordering::Relaxed);
                            let final_r = read_count.load(Ordering::Relaxed);
                            debug_assert!(final_r <= final_w);

                            drop(buffer);
                            let _ = std::fs::remove_file(&test_path);
                        },
                        BatchSize::SmallInput,
                    );
                },
            );
        }
    }
    group.finish();
}

fn bench_command_stress(c: &mut Criterion) {
    c.bench_function("command_stress", |b| {
        b.iter_batched(
            || {
                let test_path = mk_path("stress_commands");
                let _ = std::fs::remove_file(&test_path);

                let sender =
                    Arc::new(SharedRingBuffer::create(&test_path, Some(1024), Some(3000)).unwrap());
                thread::sleep(Duration::from_millis(5));
                let receiver = Arc::new(SharedRingBuffer::open(&test_path, Some(3000)).unwrap());
                (test_path, sender, receiver)
            },
            |(test_path, sender, receiver)| {
                let command_count = 1000usize;
                let sent = Arc::new(AtomicUsize::new(0));
                let recv = Arc::new(AtomicUsize::new(0));
                let barrier = Arc::new(Barrier::new(2));

                let s = sender.clone();
                let sent_c = sent.clone();
                let bar_s = barrier.clone();
                let th_s = thread::spawn(move || {
                    bar_s.wait();
                    for i in 0..command_count {
                        let cmd = SharedCommand::view_tag(1 << (i % 9), (i % 2) as i32);
                        let mut retry = 0;
                        while !s.send_command(black_box(cmd)).unwrap_or(false) {
                            retry += 1;
                            if retry > 100 {
                                break;
                            }
                            std::hint::spin_loop();
                        }
                        sent_c.fetch_add(1, Ordering::Relaxed);
                    }
                });

                let r = receiver.clone();
                let recv_c = recv.clone();
                let bar_r = barrier.clone();
                let th_r = thread::spawn(move || {
                    bar_r.wait();
                    let start = Instant::now();
                    while start.elapsed() < Duration::from_millis(500) {
                        if r.wait_for_command(Some(Duration::from_millis(1)))
                            .unwrap_or(false)
                        {
                            while let Some(_) = r.receive_command() {
                                recv_c.fetch_add(1, Ordering::Relaxed);
                            }
                        } else {
                            // 兜底拉取
                            while let Some(_) = r.receive_command() {
                                recv_c.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                });

                let _ = th_s.join();
                let _ = th_r.join();

                let final_s = sent.load(Ordering::Relaxed);
                let final_r = recv.load(Ordering::Relaxed);
                debug_assert!(final_r <= final_s);

                drop(sender);
                drop(receiver);
                let _ = std::fs::remove_file(&test_path);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_long_running_stability(c: &mut Criterion) {
    c.bench_function("long_running_stability", |b| {
        b.iter_batched(
            || {
                let test_path = mk_path("stress_long_running");
                let _ = std::fs::remove_file(&test_path);
                let buffer =
                    Arc::new(SharedRingBuffer::create(&test_path, Some(1024), Some(4000)).unwrap());
                (test_path, buffer)
            },
            |(test_path, buffer)| {
                let total_cycles = 10;
                let messages_per_cycle = 100;
                let mut msg = create_base_message(0);

                for cycle in 0..total_cycles {
                    // 写入阶段
                    for i in 0..messages_per_cycle {
                        msg.get_monitor_info_mut().monitor_num =
                            (cycle * messages_per_cycle + i) as i32;
                        let mut retry = 0;
                        while !buffer.try_write_message(&msg).unwrap_or(false) && retry < 16 {
                            let _ = buffer.try_read_next_message();
                            retry += 1;
                        }
                    }

                    // 读取阶段
                    let mut read_in_cycle = 0;
                    while let Ok(Some(_)) = buffer.try_read_next_message() {
                        read_in_cycle += 1;
                        if read_in_cycle >= messages_per_cycle {
                            break;
                        }
                    }

                    // 短暂休眠，避免过度忙等
                    thread::sleep(Duration::from_millis(1));
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
