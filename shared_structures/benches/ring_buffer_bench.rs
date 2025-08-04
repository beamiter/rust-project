// benches/ring_buffer_bench.rs
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

// 假设这是您的模块路径，请根据实际情况调整
use shared_structures::{SharedCommand, SharedMessage, SharedRingBuffer};

fn bench_single_threaded_write(c: &mut Criterion) {
    let test_path = "/tmp/bench_single_write";
    let _ = std::fs::remove_file(test_path);

    let buffer = SharedRingBuffer::create(test_path, Some(1024), Some(0)).unwrap();
    let message = create_test_message(0);

    c.bench_function("single_threaded_write", |b| {
        b.iter(|| {
            // 清空缓冲区避免满载
            while let Ok(Some(_)) = buffer.try_read_latest_message() {}

            for i in 0..100 {
                let mut msg = message;
                msg.get_monitor_info_mut().monitor_num = i;
                // 添加重试机制
                let mut retry_count = 0;
                while !buffer.try_write_message(&msg).unwrap_or(false) && retry_count < 10 {
                    // 如果缓冲区满，读取一些消息腾出空间
                    let _ = buffer.try_read_latest_message();
                    retry_count += 1;
                }
            }
        })
    });
}

fn bench_single_threaded_read(c: &mut Criterion) {
    let test_path = "/tmp/bench_single_read";
    let _ = std::fs::remove_file(test_path);

    let buffer = SharedRingBuffer::create(test_path, Some(1024), Some(0)).unwrap();
    let message = create_test_message(0);

    c.bench_function("single_threaded_read", |b| {
        b.iter(|| {
            // 预填充缓冲区
            for i in 0..100 {
                let mut msg = message;
                msg.get_monitor_info_mut().monitor_num = i;
                let mut retry_count = 0;
                while !buffer.try_write_message(&msg).unwrap_or(false) && retry_count < 10 {
                    let _ = buffer.try_read_latest_message();
                    retry_count += 1;
                }
            }

            // 读取所有消息
            while let Ok(Some(_)) = buffer.try_read_latest_message() {}
        })
    });
}

fn bench_throughput_varying_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput_by_message_count");

    for message_count in [10, 100, 1000, 10000].iter() {
        group.throughput(Throughput::Elements(*message_count as u64));
        group.bench_with_input(
            BenchmarkId::new("write_messages", message_count),
            message_count,
            |b, &count| {
                let test_path = format!("/tmp/bench_throughput_{}", count);
                let _ = std::fs::remove_file(&test_path);

                let buffer = SharedRingBuffer::create(&test_path, Some(16384), Some(0)).unwrap();
                let message = create_test_message(0);

                b.iter(|| {
                    for i in 0..count {
                        let mut msg = message;
                        msg.get_monitor_info_mut().monitor_num = i;

                        let mut retry_count = 0;
                        loop {
                            match buffer.try_write_message(&msg) {
                                Ok(true) => break,
                                Ok(false) => {
                                    // 缓冲区满，读取一条消息腾出空间
                                    let _ = buffer.try_read_latest_message();
                                    retry_count += 1;
                                    if retry_count > 100 {
                                        eprintln!("Write retry limit exceeded for message {}", i);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Write error: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_producer_consumer(c: &mut Criterion) {
    let mut group = c.benchmark_group("producer_consumer");
    group.sample_size(10); // 减少采样数量，因为这个测试比较耗时

    for spin_count in [0, 1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("adaptive_polling", spin_count),
            spin_count,
            |b, &spins| {
                b.iter(|| {
                    let test_path = format!("/tmp/bench_pc_{}", spins);
                    let _ = std::fs::remove_file(&test_path);

                    // 创建生产者缓冲区
                    let producer_buffer =
                        match SharedRingBuffer::create(&test_path, Some(1024), Some(spins)) {
                            Ok(buffer) => Arc::new(buffer),
                            Err(e) => {
                                eprintln!("Failed to create producer buffer: {}", e);
                                return;
                            }
                        };

                    // 创建消费者缓冲区，添加重试机制
                    let consumer_buffer = {
                        thread::sleep(Duration::from_millis(10)); // 确保创建完成
                        let mut attempts = 0;
                        loop {
                            match SharedRingBuffer::open(&test_path, Some(spins)) {
                                Ok(buffer) => break Arc::new(buffer),
                                Err(e) => {
                                    attempts += 1;
                                    if attempts > 10 {
                                        eprintln!(
                                            "Failed to open consumer buffer after {} attempts: {}",
                                            attempts, e
                                        );
                                        return;
                                    }
                                    thread::sleep(Duration::from_millis(10));
                                }
                            }
                        }
                    };

                    let barrier = Arc::new(Barrier::new(2));
                    let message_count = 1000u64;
                    let received_count = Arc::new(AtomicU64::new(0));
                    let sent_count = Arc::new(AtomicU64::new(0));

                    let producer_barrier = barrier.clone();
                    let consumer_barrier = barrier.clone();
                    let consumer_buffer_clone = consumer_buffer.clone();
                    let received_count_clone = received_count.clone();
                    let sent_count_clone = sent_count.clone();

                    // **关键修复**: 在移动到生产者线程之前克隆 sent_count
                    let producer_sent_count = sent_count.clone();

                    // 消费者线程
                    let consumer_handle = thread::spawn(move || {
                        consumer_barrier.wait();

                        let start_time = std::time::Instant::now();
                        let timeout_duration = Duration::from_secs(10); // 10 秒超时

                        while received_count_clone.load(Ordering::Acquire) < message_count {
                            if start_time.elapsed() > timeout_duration {
                                eprintln!(
                                    "Consumer timeout! Received: {}, Expected: {}",
                                    received_count_clone.load(Ordering::Acquire),
                                    message_count
                                );
                                break;
                            }

                            match consumer_buffer_clone
                                .wait_for_message(Some(Duration::from_millis(100)))
                            {
                                Ok(true) => {
                                    // 读取所有可用消息
                                    while let Ok(Some(_)) =
                                        consumer_buffer_clone.try_read_latest_message()
                                    {
                                        received_count_clone.fetch_add(1, Ordering::Release);
                                    }
                                }
                                Ok(false) => {
                                    // 超时，检查是否有生产者还在发送
                                    if sent_count_clone.load(Ordering::Acquire) >= message_count {
                                        // 生产者已完成，尝试读取剩余消息
                                        while let Ok(Some(_)) =
                                            consumer_buffer_clone.try_read_latest_message()
                                        {
                                            received_count_clone.fetch_add(1, Ordering::Release);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("Consumer wait error: {}", e);
                                    break;
                                }
                            }
                        }
                    });

                    // 生产者线程
                    let producer_buffer_clone = producer_buffer.clone();
                    let producer_handle = thread::spawn(move || {
                        producer_barrier.wait();
                        let message = create_test_message(0);

                        for i in 0..message_count {
                            let mut msg = message;
                            msg.get_monitor_info_mut().monitor_num = i as i32;

                            let mut retry_count = 0;
                            let start_time = std::time::Instant::now();

                            loop {
                                if start_time.elapsed() > Duration::from_secs(5) {
                                    eprintln!("Producer timeout on message {}", i);
                                    return;
                                }

                                match producer_buffer_clone.try_write_message(&msg) {
                                    Ok(true) => {
                                        producer_sent_count.fetch_add(1, Ordering::Release);
                                        break;
                                    }
                                    Ok(false) => {
                                        // 缓冲区满，短暂等待
                                        thread::sleep(Duration::from_micros(100));
                                        retry_count += 1;
                                        if retry_count > 10000 {
                                            eprintln!(
                                                "Producer giving up on message {} after {} retries",
                                                i, retry_count
                                            );
                                            return;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Producer error on message {}: {}", i, e);
                                        return;
                                    }
                                }
                            }
                        }
                    });

                    // 等待线程完成
                    let producer_result = producer_handle.join();
                    let consumer_result = consumer_handle.join();

                    if let Err(_) = producer_result {
                        eprintln!("Producer thread panicked");
                    }
                    if let Err(_) = consumer_result {
                        eprintln!("Consumer thread panicked");
                    }

                    // 检查结果 - 现在可以安全地访问 sent_count
                    let final_sent = sent_count.load(Ordering::Acquire);
                    let final_received = received_count.load(Ordering::Acquire);

                    if final_sent != message_count || final_received != message_count {
                        eprintln!(
                            "Message count mismatch - Sent: {}, Received: {}, Expected: {}",
                            final_sent, final_received, message_count
                        );
                    }

                    // 显式清理
                    drop(producer_buffer);
                    drop(consumer_buffer);

                    // 确保文件被删除
                    let _ = std::fs::remove_file(&test_path);
                });
            },
        );
    }
    group.finish();
}

fn bench_command_latency(c: &mut Criterion) {
    let test_path = "/tmp/bench_cmd_latency";
    let _ = std::fs::remove_file(test_path);

    let sender = match SharedRingBuffer::create(test_path, Some(1024), Some(1000)) {
        Ok(buffer) => buffer,
        Err(e) => {
            eprintln!("Failed to create sender buffer: {}", e);
            return;
        }
    };

    thread::sleep(Duration::from_millis(10)); // 确保创建完成

    let receiver = match SharedRingBuffer::open(test_path, Some(1000)) {
        Ok(buffer) => buffer,
        Err(e) => {
            eprintln!("Failed to open receiver buffer: {}", e);
            return;
        }
    };

    c.bench_function("command_round_trip", |b| {
        b.iter(|| {
            let command = SharedCommand::view_tag(1 << 3, 0);

            // 发送命令
            match sender.send_command(command) {
                Ok(true) => {}
                Ok(false) => {
                    eprintln!("Command buffer full");
                    return;
                }
                Err(e) => {
                    eprintln!("Send command error: {}", e);
                    return;
                }
            }

            // 等待并接收命令
            match receiver.wait_for_command(Some(Duration::from_millis(100))) {
                Ok(true) => {
                    if let Some(_cmd) = receiver.receive_command() {
                        // 成功接收命令
                    } else {
                        eprintln!("No command received despite signal");
                    }
                }
                Ok(false) => {
                    eprintln!("Wait for command timed out");
                }
                Err(e) => {
                    eprintln!("Wait for command error: {}", e);
                }
            }
        })
    });
}

fn bench_memory_layout_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_layout");

    // 测试不同缓冲区大小的性能
    for &buffer_size in [16, 64, 256, 1024, 4096].iter() {
        if (buffer_size as u32).is_power_of_two() {
            group.bench_with_input(
                BenchmarkId::new("buffer_size", buffer_size),
                &buffer_size,
                |b, &size| {
                    let test_path = format!("/tmp/bench_layout_{}", size);
                    let _ = std::fs::remove_file(&test_path);

                    let buffer = match SharedRingBuffer::create(&test_path, Some(size), Some(1000))
                    {
                        Ok(buffer) => buffer,
                        Err(e) => {
                            eprintln!("Failed to create buffer for size {}: {}", size, e);
                            return;
                        }
                    };

                    let message = create_test_message(0);

                    b.iter(|| {
                        // 填充到接近满载
                        let target_count = (size * 3) / 4; // 75% 填充
                        for i in 0..target_count {
                            let mut msg = message;
                            msg.get_monitor_info_mut().monitor_num = i as i32;

                            match buffer.try_write_message(&msg) {
                                Ok(true) => {}
                                Ok(false) => break, // 缓冲区满
                                Err(e) => {
                                    eprintln!("Write error during fill: {}", e);
                                    break;
                                }
                            }
                        }

                        // 交替读写模拟真实场景
                        for i in 0..100 {
                            // 尝试读取
                            match buffer.try_read_latest_message() {
                                Ok(Some(_)) => {}
                                Ok(None) => {} // 没有消息
                                Err(e) => eprintln!("Read error: {}", e),
                            }

                            // 尝试写入
                            let mut msg = message;
                            msg.get_monitor_info_mut().monitor_num = 999 + i;
                            match buffer.try_write_message(&msg) {
                                Ok(true) => {}
                                Ok(false) => {
                                    // 缓冲区满，先读取再重试
                                    let _ = buffer.try_read_latest_message();
                                    let _ = buffer.try_write_message(&msg);
                                }
                                Err(e) => eprintln!("Write error during alternation: {}", e),
                            }
                        }
                    });
                },
            );
        }
    }
    group.finish();
}

fn bench_burst_performance(c: &mut Criterion) {
    let mut group = c.benchmark_group("burst_performance");

    for &burst_size in [10, 50, 100, 500].iter() {
        group.bench_with_input(
            BenchmarkId::new("burst_write_read", burst_size),
            &burst_size,
            |b, &size| {
                let test_path = format!("/tmp/bench_burst_{}", size);
                let _ = std::fs::remove_file(&test_path);

                let buffer = match SharedRingBuffer::create(&test_path, Some(2048), Some(0)) {
                    Ok(buffer) => buffer,
                    Err(e) => {
                        eprintln!("Failed to create burst buffer: {}", e);
                        return;
                    }
                };

                let message = create_test_message(0);

                b.iter(|| {
                    // 突发写入
                    for i in 0..size {
                        let mut msg = message;
                        msg.get_monitor_info_mut().monitor_num = i;

                        let mut retry_count = 0;
                        while !buffer.try_write_message(&msg).unwrap_or(false) && retry_count < 10 {
                            let _ = buffer.try_read_latest_message();
                            retry_count += 1;
                        }
                    }

                    // 突发读取
                    let mut read_count = 0;
                    while let Ok(Some(_)) = buffer.try_read_latest_message() {
                        read_count += 1;
                        if read_count >= size {
                            break;
                        }
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_adaptive_polling_effectiveness(c: &mut Criterion) {
    let mut group = c.benchmark_group("adaptive_polling");

    for &spin_count in [0, 100, 1000, 5000, 10000].iter() {
        group.bench_with_input(
            BenchmarkId::new("spin_effectiveness", spin_count),
            &spin_count,
            |b, &spins| {
                let test_path = format!("/tmp/bench_adaptive_{}", spins);
                let _ = std::fs::remove_file(&test_path);

                let writer = match SharedRingBuffer::create(&test_path, Some(512), Some(spins)) {
                    Ok(buffer) => Arc::new(buffer),
                    Err(e) => {
                        eprintln!("Failed to create adaptive writer: {}", e);
                        return;
                    }
                };

                thread::sleep(Duration::from_millis(10));

                let reader = match SharedRingBuffer::open(&test_path, Some(spins)) {
                    Ok(buffer) => Arc::new(buffer),
                    Err(e) => {
                        eprintln!("Failed to open adaptive reader: {}", e);
                        return;
                    }
                };

                b.iter(|| {
                    let writer_clone = writer.clone();
                    let reader_clone = reader.clone();
                    let barrier = Arc::new(Barrier::new(2));
                    let barrier_clone = barrier.clone();

                    // 写入线程
                    let writer_handle = thread::spawn(move || {
                        barrier_clone.wait();
                        let message = create_test_message(42);

                        match writer_clone.try_write_message(&message) {
                            Ok(true) => {}
                            Ok(false) => eprintln!("Write failed - buffer full"),
                            Err(e) => eprintln!("Write error: {}", e),
                        }
                    });

                    // 读取线程
                    let reader_handle = thread::spawn(move || {
                        barrier.wait();

                        match reader_clone.wait_for_message(Some(Duration::from_millis(100))) {
                            Ok(true) => {
                                let _ = reader_clone.try_read_latest_message();
                            }
                            Ok(false) => eprintln!("Wait timeout"),
                            Err(e) => eprintln!("Wait error: {}", e),
                        }
                    });

                    let _ = writer_handle.join();
                    let _ = reader_handle.join();
                });
            },
        );
    }
    group.finish();
}

// 辅助函数
fn create_test_message(id: i32) -> SharedMessage {
    let mut message = SharedMessage::default();
    message.get_monitor_info_mut().monitor_num = id;
    message
        .get_monitor_info_mut()
        .set_client_name(&format!("test_client_{}", id));
    message.get_monitor_info_mut().set_ltsymbol("[]=");
    message
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
    bench_adaptive_polling_effectiveness
);

criterion_main!(benches);
