# Install performance tools
sudo apt-get install -y htop iotop nethogs linux-tools-common linux-tools-generic sysbench

# Network optimizations (these should work in cloud)
sudo sysctl -w net.core.rmem_max=134217728
sudo sysctl -w net.core.wmem_max=134217728
sudo sysctl -w net.ipv4.tcp_rmem="4096 87380 134217728"
sudo sysctl -w net.ipv4.tcp_wmem="4096 65536 134217728"
sudo sysctl -w net.core.netdev_max_backlog=30000
sudo sysctl -w net.ipv4.tcp_congestion_control=bbr
sudo sysctl -w net.ipv4.tcp_mtu_probing=1
sudo sysctl -w net.core.default_qdisc=fq

# File descriptor limits
sudo bash -c 'echo "* soft nofile 1000000" >> /etc/security/limits.conf'
sudo bash -c 'echo "* hard nofile 1000000" >> /etc/security/limits.conf'
sudo bash -c 'echo "fs.file-max = 1000000" >> /etc/sysctl.conf'

# Apply sysctl changes
sudo sysctl -p

# Verify settings
./scripts/validate_amd_genoa_cloud.sh
