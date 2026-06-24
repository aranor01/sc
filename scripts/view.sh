#!/bin/bash
if [ -n "$PAGER" ]; then
    exec "$PAGER" "$@"
fi
for pager in less more; do
    if command -v "$pager" >/dev/null 2>&1; then
        exec "$pager" "$@"
    fi
done
echo "No pager found. Set the PAGER environment variable (e.g. export PAGER=less)." >&2
exit 1
