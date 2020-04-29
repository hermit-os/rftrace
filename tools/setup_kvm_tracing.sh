#!/bin/sh
echo "Letting all users read the debugfs tracing directory!"
sudo chmod a+rx /sys/kernel/debug
sleep 1
sudo chmod a+rx /sys/kernel/debug/tracing

echo "Setting up kvm_write_tsc_offset!"
cd /sys/kernel/debug/tracing/instances
sudo mkdir tsc_offset
cd tsc_offset
echo x86-tsc | sudo tee trace_clock # so we know at which host-time the guest changed the timestamp.
echo 1 | sudo tee events/kvm/kvm_write_tsc_offset/enable

echo "Read offset from /sys/kernel/debug/tracing/instances/tsc_offset/trace"
