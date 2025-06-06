#!/bin/bash

echo "=== AMD Genoa CPU Validation (Cloud Environment) ==="
echo "CPU Model:"
lscpu | grep "Model name"
echo "CPU Count: $(nproc)"
echo "CPU MHz: $(grep "cpu MHz" /proc/cpuinfo | head -1)"

echo -e "\n=== Zen 4 Features ==="
echo "AVX-512 Support:"
grep -o 'avx512[a-z_]*' /proc/cpuinfo | sort -u | head -10

echo -e "\n=== Rust Target Features ==="
if command -v rustc &> /dev/null; then
    rustc --print target-features -C target-cpu=znver4 2>&1 | grep -E "(enabled|znver)" | head -15
    if [ ${PIPESTATUS[0]} -ne 0 ]; then
        echo "Note: znver4 may not be available in this Rust version, trying znver3..."
        rustc --print target-features -C target-cpu=znver3 | grep enabled | head -10
    fi
else
    echo "Rust not installed. Run: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

echo -e "\n=== NUMA Configuration ==="
numactl --hardware | head -10

echo -e "\n=== Memory Info ==="
free -h
echo "Transparent Huge Pages: $(cat /sys/kernel/mm/transparent_hugepage/enabled)"

echo -e "\n=== Current System Limits ==="
ulimit -n  # File descriptors
ulimit -u  # Max processes

echo -e "\n=== Network Configuration ==="
sysctl net.core.rmem_max net.core.wmem_max net.ipv4.tcp_rmem net.ipv4.tcp_wmem 2>/dev/null

echo -e "\n=== Compiler Optimization Check ==="
if command -v rustc &> /dev/null; then
    echo 'fn main() { println!("AMD Genoa optimized!"); }' > /tmp/test.rs
    
    # Try znver4 first
    if rustc -C target-cpu=znver4 --emit asm -o /tmp/test_znver4.s /tmp/test.rs 2>/dev/null; then
        echo "✓ znver4 target CPU supported"
        grep -E "(vzeroupper|vpxor|vmovdqu)" /tmp/test_znver4.s > /dev/null && echo "✓ AVX-512 instructions detected in znver4 build"
    else
        echo "! znver4 not supported, trying znver3..."
        if rustc -C target-cpu=znver3 --emit asm -o /tmp/test_znver3.s /tmp/test.rs 2>/dev/null; then
            echo "✓ znver3 target CPU supported (will use this instead)"
            grep -E "(vzeroupper|vpxor|vmovdqu)" /tmp/test_znver3.s > /dev/null && echo "✓ AVX instructions detected in znver3 build"
        fi
    fi
    
    # Also try native
    rustc -C target-cpu=native --emit asm -o /tmp/test_native.s /tmp/test.rs 2>/dev/null
    echo "✓ native target CPU detection"
    
    rm -f /tmp/test*.rs /tmp/test*.s
fi
