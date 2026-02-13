#!/bin/bash
# Auto-run cargo check after editing Rust files

INPUT=$(cat)
FILE_PATH=$(echo "$INPUT" | jq -r '.tool_input.file_path')

if [[ "$FILE_PATH" == *.rs ]]; then
  cargo check --quiet --no-default-features 2>&1 | head -20
fi

exit 0
