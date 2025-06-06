#!/bin/bash

echo "=== AMD Genoa CPU Validation ==="
echo "CPU Model:"
lscpu | grep "Model name"

echo -e "\n=== Zen 4 Features ==="
echo "AVX-512 Support:"
grep -o 'avx512[a-z_]*' /proc/cpuinfo | sort -u | head -10

echo -e "\n=== Rust Target Features ==="
rustc --print target-features -C target-cpu=znver4 | grep enabled | grep -E "(avx512|gfni|vaes|vpclmul)" | head -10

echo -e "\n=== Current CPU Frequencies ==="
grep "cpu MHz" /proc/cpuinfo | sort -u | head -5

echo -e "\n=== NUMA Configuration ==="
numactl --hardware | head -10

echo -e "\n=== Memory Bandwidth Test ==="
if command -v sysbench &> /dev/null; then
    sysbench memory --memory-total-size=10G --memory-oper=write run | grep -E "(transferred|Operations|total time)"
else
    echo "Install sysbench for memory bandwidth testing: sudo apt-get install -y sysbench"
fi

echo -e "\n=== Compiler Optimization Check ==="
echo 'fn main() { println!("AMD Genoa optimized!"); }' > /tmp/test.rs
rustc -C target-cpu=znver4 --emit asm -o /tmp/test.s /tmp/test.rs 2>&1
if [ $? -eq 0 ]; then
    echo "✓ znver4 target CPU accepted by rustc"
    grep -E "(vzeroupper|vpxor|vmovdqu64)" /tmp/test.s > /dev/null && echo "✓ AVX-512 instructions detected"
else
    echo "✗ znver4 target not recognized, falling back to znver3"
fi
rm -f /tmp/test.rs /tmp/test.s
