#!/usr/bin/env bash
# Fails if any .rs file in src/ exceeds MAX lines
MAX=500
fail=0
while IFS= read -r -d '' file; do
  count=$(wc -l < "$file")
  if (( count > MAX )); then
    echo "FAIL: $file has $count lines (limit: $MAX)"
    fail=1
  fi
done < <(find src -name '*.rs' -print0)
exit $fail
