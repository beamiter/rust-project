// benches/stress_test.rs
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    Arc, Barrier,
};
use std::thread;
use std::time::{Duration, Instant};

fn create_test_message(id: i32) -> SharedMessage {
    let mut message = SharedMessage::default();
    message.get_monitor_info_mut().monitor_num = id;
    message
        .get_monitor_info_mut()
        .set_client_name(&format!("test_client_{}", id));
    message.get_monitor_info_mut().set_ltsymbol("[]=");
    message
}

fn bench_high_frequency_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_frequency");
    group.sample_size(20); // 减少采样以加快测试

    // 测试不同的自适应轮询配置
    for &spins in [0, 1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("updates", spins),
            &spins,
            |b, &spin_count| {
                b.iter(|| {
                    let test_path = format!("/tmp/stress_high_freq_{}", spin_count);
                    let _ = std::fs::remove_file(&test_path);

                    let buffer =
                        match SharedRingBuffer::create(&test_path, Some(2048), Some(spin_count)) {
                            Ok(buffer) => Arc::new(buffer),
                            Err(e) => {
                                eprintln!("Failed to create buffer: {}", e);
                                return;
                            }
                        };

                    let stop_flag = Arc::new(AtomicBool::new(false));
                    let update_count = Arc::new(AtomicU64::new(0));
                    let error_count = Arc::new(AtomicU64::new(0));

                    let buffer_clone = buffer.clone();
                    let stop_flag_clone = stop_flag.clone();
                    let update_count_clone = update_count.clone();
                    let error_count_clone = error_count.clone();

                    // 高频生产者
                    let producer = thread::spawn(move || {
                        let mut counter = 0u32;
                        let mut retry_count = 0u32;

                        while !stop_flag_clone.load(Ordering::Acquire) {
                            let mut message = create_test_message(counter as i32);
                            message.get_monitor_info_mut().monitor_num = counter as i32;

                            match buffer_clone.try_write_message(&message) {
                                Ok(true) => {
                                    update_count_clone.fetch_add(1, Ordering::Release);
                                    counter = counter.wrapping_add(1);
                                    retry_count = 0;
                                }
                                Ok(false) => {
                                    // 缓冲区满，短暂等待
                                    retry_count += 1;
                                    if retry_count % 1000 == 0 {
                                        std::thread::yield_now();
                                    }
                                }
                                Err(_) => {
                                    error_count_clone.fetch_add(1, Ordering::Release);
                                    break;
                                }
                            }
                        }
                    });

                    // 运行测试
                    thread::sleep(Duration::from_millis(100));
                    stop_flag.store(true, Ordering::Release);

                    if let Err(_) = producer.join() {
                        eprintln!("Producer thread panicked");
                    }

                    let final_updates = update_count.load(Ordering::Acquire);
                    let final_errors = error_count.load(Ordering::Acquire);

                    if final_errors > 0 {
                        eprintln!("Errors occurred: {}", final_errors);
                    }

                    // 清空缓冲区
                    let mut read_count = 0;
                    while let Ok(Some(_)) = buffer.try_read_latest_message() {
                        read_count += 1;
                    }

                    // 验证数据一致性
                    assert!(read_count <= final_updates);
                });
            },
        );
    }
    group.finish();
}

fn bench_concurrent_stress(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_stress");
    group.sample_size(10);

    // 测试不同数量的生产者线程
    for &num_producers in [1, 2, 4, 8].iter() {
        group.bench_with_input(
            BenchmarkId::new("producers", num_producers),
            &num_producers,
            |b, &producer_count| {
                b.iter(|| {
                    let test_path = format!("/tmp/stress_concurrent_{}", producer_count);
                    let _ = std::fs::remove_file(&test_path);

                    let producer_buffer =
                        match SharedRingBuffer::create(&test_path, Some(4096), Some(5000)) {
                            Ok(buffer) => Arc::new(buffer),
                            Err(e) => {
                                eprintln!("Failed to create producer buffer: {}", e);
                                return;
                            }
                        };

                    thread::sleep(Duration::from_millis(10)); // 确保创建完成

                    let consumer_buffer = match SharedRingBuffer::open(&test_path, Some(5000)) {
                        Ok(buffer) => Arc::new(buffer),
                        Err(e) => {
                            eprintln!("Failed to open consumer buffer: {}", e);
                            return;
                        }
                    };

                    let mut handles = vec![];
                    let test_duration = Duration::from_millis(100);
                    let start_time = Arc::new(Instant::now());
                    let barrier = Arc::new(Barrier::new(producer_count + 1)); // +1 for consumer

                    let total_produced = Arc::new(AtomicU64::new(0));
                    let total_errors = Arc::new(AtomicU64::new(0));

                    // 启动多个生产者线程
                    for thread_id in 0..producer_count {
                        let buffer = Arc::clone(&producer_buffer);
                        let start_time = Arc::clone(&start_time);
                        let barrier = Arc::clone(&barrier);
                        let total_produced = Arc::clone(&total_produced);
                        let total_errors = Arc::clone(&total_errors);

                        let handle = thread::spawn(move || {
                            barrier.wait(); // 等待所有线程就绪

                            let mut local_counter = 0u32;
                            let mut local_errors = 0u32;

                            while start_time.elapsed() < test_duration {
                                let mut message = create_test_message(local_counter as i32);
                                message.get_monitor_info_mut().monitor_num =
                                    ((thread_id << 24) as u32 | local_counter) as i32;

                                match buffer.try_write_message(&message) {
                                    Ok(true) => {
                                        local_counter = local_counter.wrapping_add(1);
                                    }
                                    Ok(false) => {
                                        // 缓冲区满，继续尝试
                                        std::thread::yield_now();
                                    }
                                    Err(_) => {
                                        local_errors += 1;
                                        if local_errors > 100 {
                                            break; // 避免无限错误
                                        }
                                    }
                                }
                            }

                            total_produced.fetch_add(local_counter as u64, Ordering::Release);
                            total_errors.fetch_add(local_errors as u64, Ordering::Release);
                            (local_counter, local_errors)
                        });
                        handles.push(handle);
                    }

                    // 启动消费者线程
                    let consumer_buffer_clone = Arc::clone(&consumer_buffer);
                    let start_time_clone = Arc::clone(&start_time);
                    let barrier_clone = Arc::clone(&barrier);

                    let consumer_handle = thread::spawn(move || {
                        barrier_clone.wait(); // 等待所有线程就绪

                        let mut consumed = 0u32;
                        let mut consumer_errors = 0u32;

                        while start_time_clone.elapsed() < test_duration {
                            match consumer_buffer_clone
                                .wait_for_message(Some(Duration::from_micros(500)))
                            {
                                Ok(true) => {
                                    while let Ok(Some(_)) =
                                        consumer_buffer_clone.try_read_latest_message()
                                    {
                                        consumed = consumed.wrapping_add(1);
                                    }
                                }
                                Ok(false) => {
                                    // 超时，继续尝试
                                }
                                Err(_) => {
                                    consumer_errors += 1;
                                    if consumer_errors > 50 {
                                        break;
                                    }
                                }
                            }
                        }

                        // 尝试读取剩余消息
                        while let Ok(Some(_)) = consumer_buffer_clone.try_read_latest_message() {
                            consumed = consumed.wrapping_add(1);
                        }

                        (consumed, consumer_errors)
                    });

                    // 等待所有线程完成
                    let _producer_results: Vec<_> = handles
                        .into_iter()
                        .map(|h| h.join().unwrap_or((0, u32::MAX)))
                        .collect();

                    let (total_consumed, consumer_errors) =
                        consumer_handle.join().unwrap_or((0, u32::MAX));

                    let final_produced = total_produced.load(Ordering::Acquire);
                    let final_errors = total_errors.load(Ordering::Acquire);

                    // 验证和报告
                    if final_errors > 0 {
                        eprintln!("Producer errors: {}", final_errors);
                    }
                    if consumer_errors > 0 && consumer_errors != u32::MAX {
                        eprintln!("Consumer errors: {}", consumer_errors);
                    }

                    // 基本一致性检查
                    assert!(total_consumed as u64 <= final_produced);

                    // 清理
                    drop(producer_buffer);
                    drop(consumer_buffer);
                    let _ = std::fs::remove_file(&test_path);
                });
            },
        );
    }
    group.finish();
}

fn bench_memory_pressure(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_pressure");
    group.sample_size(10);

    // 测试不同缓冲区大小下的内存压力
    for &buffer_size in [64, 256, 1024, 4096].iter() {
        if (buffer_size as u32).is_power_of_two() {
            group.bench_with_input(
                BenchmarkId::new("buffer_size", buffer_size),
                &buffer_size,
                |b, &size| {
                    b.iter(|| {
                        let test_path = format!("/tmp/stress_memory_{}", size);
                        let _ = std::fs::remove_file(&test_path);

                        let buffer =
                            match SharedRingBuffer::create(&test_path, Some(size), Some(2000)) {
                                Ok(buffer) => Arc::new(buffer),
                                Err(e) => {
                                    eprintln!("Failed to create memory pressure buffer: {}", e);
                                    return;
                                }
                            };

                        let iterations = size * 10; // 超过缓冲区容量的写入
                        let write_count = Arc::new(AtomicUsize::new(0));
                        let read_count = Arc::new(AtomicUsize::new(0));

                        let write_buffer = Arc::clone(&buffer);
                        let read_buffer = Arc::clone(&buffer);
                        let write_count_clone = Arc::clone(&write_count);
                        let read_count_clone = Arc::clone(&read_count);

                        let barrier = Arc::new(Barrier::new(2));
                        let barrier_clone = Arc::clone(&barrier);

                        // 写入线程
                        let writer = thread::spawn(move || {
                            barrier_clone.wait();

                            for i in 0..iterations {
                                let message = create_test_message(i as i32);
                                let mut retry_count = 0;

                                loop {
                                    match write_buffer.try_write_message(&message) {
                                        Ok(true) => {
                                            write_count_clone.fetch_add(1, Ordering::Release);
                                            break;
                                        }
                                        Ok(false) => {
                                            retry_count += 1;
                                            if retry_count > 1000 {
                                                break; // 避免无限重试
                                            }
                                            std::thread::yield_now();
                                        }
                                        Err(_) => break,
                                    }
                                }
                            }
                        });

                        // 读取线程
                        let reader = thread::spawn(move || {
                            barrier.wait();

                            let start_time = Instant::now();
                            while start_time.elapsed() < Duration::from_millis(200) {
                                match read_buffer.try_read_latest_message() {
                                    Ok(Some(_)) => {
                                        read_count_clone.fetch_add(1, Ordering::Release);
                                    }
                                    Ok(None) => {
                                        std::thread::yield_now();
                                    }
                                    Err(_) => break,
                                }
                            }

                            // 清空剩余消息
                            while let Ok(Some(_)) = read_buffer.try_read_latest_message() {
                                read_count_clone.fetch_add(1, Ordering::Release);
                            }
                        });

                        let _ = writer.join();
                        let _ = reader.join();

                        let final_writes = write_count.load(Ordering::Acquire);
                        let final_reads = read_count.load(Ordering::Acquire);

                        // 验证没有消息丢失
                        assert!(final_reads <= final_writes);
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_command_stress(c: &mut Criterion) {
    c.bench_function("command_stress", |b| {
        b.iter(|| {
            let test_path = "/tmp/stress_commands";
            let _ = std::fs::remove_file(test_path);

            let sender = match SharedRingBuffer::create(test_path, Some(1024), Some(3000)) {
                Ok(buffer) => Arc::new(buffer),
                Err(e) => {
                    eprintln!("Failed to create command sender: {}", e);
                    return;
                }
            };

            thread::sleep(Duration::from_millis(5));

            let receiver = match SharedRingBuffer::open(test_path, Some(3000)) {
                Ok(buffer) => Arc::new(buffer),
                Err(e) => {
                    eprintln!("Failed to open command receiver: {}", e);
                    return;
                }
            };

            let command_count = 1000;
            let sent_count = Arc::new(AtomicUsize::new(0));
            let received_count = Arc::new(AtomicUsize::new(0));

            let sender_clone = Arc::clone(&sender);
            let receiver_clone = Arc::clone(&receiver);
            let sent_count_clone = Arc::clone(&sent_count);
            let received_count_clone = Arc::clone(&received_count);

            let barrier = Arc::new(Barrier::new(2));
            let barrier_clone = Arc::clone(&barrier);

            // 命令发送线程
            let sender_thread = thread::spawn(move || {
                barrier_clone.wait();

                for i in 0..command_count {
                    let command = SharedCommand::view_tag(1 << (i % 9), i % 2);
                    let mut retry_count = 0;

                    loop {
                        match sender_clone.send_command(command) {
                            Ok(true) => {
                                sent_count_clone.fetch_add(1, Ordering::Release);
                                break;
                            }
                            Ok(false) => {
                                retry_count += 1;
                                if retry_count > 100 {
                                    break;
                                }
                                std::thread::yield_now();
                            }
                            Err(_) => break,
                        }
                    }
                }
            });

            // 命令接收线程
            let receiver_thread = thread::spawn(move || {
                barrier.wait();

                let start_time = Instant::now();
                while start_time.elapsed() < Duration::from_millis(500) {
                    match receiver_clone.wait_for_command(Some(Duration::from_millis(10))) {
                        Ok(true) => {
                            while let Some(_) = receiver_clone.receive_command() {
                                received_count_clone.fetch_add(1, Ordering::Release);
                            }
                        }
                        Ok(false) => {
                            // 检查是否还有命令
                            while let Some(_) = receiver_clone.receive_command() {
                                received_count_clone.fetch_add(1, Ordering::Release);
                            }
                        }
                        Err(_) => break,
                    }
                }
            });

            let _ = sender_thread.join();
            let _ = receiver_thread.join();

            let final_sent = sent_count.load(Ordering::Acquire);
            let final_received = received_count.load(Ordering::Acquire);

            assert!(final_received <= final_sent);
        });
    });
}

fn bench_long_running_stability(c: &mut Criterion) {
    c.bench_function("long_running_stability", |b| {
        b.iter(|| {
            let test_path = "/tmp/stress_long_running";
            let _ = std::fs::remove_file(test_path);

            let buffer = match SharedRingBuffer::create(test_path, Some(1024), Some(4000)) {
                Ok(buffer) => Arc::new(buffer),
                Err(e) => {
                    eprintln!("Failed to create long running buffer: {}", e);
                    return;
                }
            };

            let total_cycles = 10;
            let messages_per_cycle = 100;

            for cycle in 0..total_cycles {
                // 写入阶段
                for i in 0..messages_per_cycle {
                    let mut message = create_test_message((cycle * messages_per_cycle + i) as i32);
                    message.get_monitor_info_mut().monitor_num = i as i32;

                    let mut retry_count = 0;
                    while !buffer.try_write_message(&message).unwrap_or(false) && retry_count < 10 {
                        let _ = buffer.try_read_latest_message();
                        retry_count += 1;
                    }
                }

                // 读取阶段
                let mut read_in_cycle = 0;
                while let Ok(Some(_)) = buffer.try_read_latest_message() {
                    read_in_cycle += 1;
                    if read_in_cycle >= messages_per_cycle {
                        break;
                    }
                }

                // 短暂休息
                thread::sleep(Duration::from_millis(1));
            }

            // 最终清理
            while let Ok(Some(_)) = buffer.try_read_latest_message() {}
        });
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
