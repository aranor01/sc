#!/usr/bin/env bash
# sc-complete — Tab completion helper for Sunset Commander
# Usage: sc-complete <cmdline>
# Prints one completion candidate per line on stdout.
# Completes from the end of the input (cursor-at-end assumed).
# Exits 0 on success; any non-zero exit is silently ignored by the UI.

# Source bash-completion for _completion_loader support (non-fatal if absent)
source /usr/share/bash-completion/bash_completion 2>/dev/null || true

cmdline="${1-}"

# Shell-quote a completion candidate for safe insertion into the cmdline,
# without escaping a trailing space some completion functions (e.g. git's)
# append to mark a word as complete — that space is a word separator, not
# part of the filename/word itself.
quote_candidate() {
    local s="$1"
    if [[ "$s" == *' ' ]]; then
        printf '%q \n' "${s% }"
    else
        printf '%q\n' "$s"
    fi
}

# Split into words; note whether input ends with whitespace (new word in progress)
read -ra words <<< "$cmdline"
n=${#words[@]}

if [[ "$cmdline" =~ [[:space:]]$ ]] || [ "$n" -eq 0 ]; then
    cur=""
    trailing_space=true
else
    cur="${words[$((n-1))]}"
    trailing_space=false
fi

# Empty cmdline → complete command names from empty prefix
if [ "$n" -eq 0 ]; then
    compgen -c -- "" | { sort -u 2>/dev/null || cat; }
    exit 0
fi

# Special-prefix completions (apply regardless of word position)
if [[ "$cur" == ~* ]]; then
    compgen -u -- "${cur#\~}" | sed 's/^/~/'
    exit 0
fi
if [[ "$cur" == '$'* ]]; then
    compgen -v -- "${cur#'$'}" | sed 's/^/\$/'
    exit 0
fi
if [[ "$cur" == @* ]]; then
    compgen -A hostname -- "${cur#@}" | sed 's/^/@/'
    exit 0
fi

# First word (no space yet): complete command names
if [ "$n" -eq 1 ] && [ "$trailing_space" = false ]; then
    compgen -c -- "$cur" | { sort -u 2>/dev/null || cat; }
    exit 0
fi

# Subsequent words: try command-specific completion, fall back to filenames
cmd="${words[0]}"

if declare -f _completion_loader &>/dev/null; then
    _completion_loader "$cmd" 2>/dev/null

    func=$(complete -p "$cmd" 2>/dev/null | grep -oP '(?<=-F )\S+')

    if [ -n "$func" ]; then
        COMP_LINE="$cmdline"
        COMP_POINT=${#cmdline}
        if [ "$trailing_space" = true ]; then
            COMP_WORDS=("${words[@]}" "")
        else
            COMP_WORDS=("${words[@]}")
        fi
        COMP_CWORD=$(( ${#COMP_WORDS[@]} - 1 ))

        if [ "$COMP_CWORD" -gt 0 ]; then
            prev="${COMP_WORDS[$((COMP_CWORD-1))]}"
        else
            prev=""
        fi

        COMPREPLY=()
        "$func" "$cmd" "$cur" "$prev" 2>/dev/null

        if [ "${#COMPREPLY[@]}" -gt 0 ]; then
            printf '%s\n' "${COMPREPLY[@]}" | while IFS= read -r f; do quote_candidate "$f"; done
            exit 0
        fi
    fi
fi

# Fallback: filename completion
compgen -f -- "$cur" | while IFS= read -r f; do printf '%q\n' "$f"; done
