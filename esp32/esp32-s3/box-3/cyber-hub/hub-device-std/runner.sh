#!/bin/bash
# Cyber-Hub Automated Runner
# $1 is the ELF binary passed by Cargo

if [ -z "$1" ]; then
    echo "Usage: $0 <ELF_PATH>"
    exit 1
fi

ELF_PATH=$1
MODEL_BIN="srmodels.bin"

if [ -f "$MODEL_BIN" ]; then
    echo "📦 Step 1: Writing voice models to 0x810000..."
    # Flash models and inhibit reset to keep serial connection for the next command
    espflash write-bin --after no-reset 0x810000 "$MODEL_BIN"
else
    echo "⚠️ Warning: $MODEL_BIN not found, skipping model flash."
fi

echo "🚀 Step 2: Flashing application and starting monitor..."
# Execute standard flash for the ELF and start monitoring
espflash flash --monitor --flash-size 16mb --partition-table partitions.csv "$ELF_PATH"
