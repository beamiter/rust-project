#!/bin/bash
# benchmark.sh

echo "🚀 Starting comprehensive benchmark suite..."

# 基础性能测试
echo "📊 Running basic performance benchmarks..."
cargo bench --bench ring_buffer_bench

# 压力测试
echo "💪 Running stress tests..."
cargo bench --bench stress_test

# 生成性能报告
echo "📈 Generating performance reports..."
echo "Results saved to target/criterion/"

# 可选：运行内存使用分析
echo "🔍 Memory usage analysis..."
valgrind --tool=massif --stacks=yes cargo bench --bench ring_buffer_bench > memory_report.txt 2>&1

echo "✅ Benchmark suite completed!"
echo "📁 Check target/criterion/ for detailed HTML reports"

# # 运行所有基准测试
# cargo bench
#
# # 只运行特定基准测试
# cargo bench -- "single_threaded"
#
# # 生成 HTML 报告
# cargo bench --bench ring_buffer_bench -- --output-format html
#
# # 对比不同版本的性能
# cargo bench -- --save-baseline before_optimization
# # 修改代码后
# cargo bench -- --baseline before_optimization
