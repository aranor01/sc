#!/bin/bash
if [ -n "$EDITOR" ]; then
    exec "$EDITOR" "$@"
fi
for editor in vim nano vi; do
    if command -v "$editor" >/dev/null 2>&1; then
        exec "$editor" "$@"
    fi
done
echo "No editor found. Set the EDITOR environment variable (e.g. export EDITOR=nano)." >&2
exit 1
