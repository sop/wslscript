#!/bin/bash
# This script has unicode characters, whitespace and single quotes in its filename.

printf 'This script'\''s filename is "%s"\n' "$0"
printf 'Canonical path is "%s"\n' "$(readlink -f "$0")"
argv=("$0" "$@")
argc=${#argv[@]}
for ((i = 0; i < $argc; i++)); do
    arg="${argv[$i]}"
    printf 'Argument #%d: %s\n' "$i" "$arg"
    if [[ -e "$arg" ]]; then
        stat "$arg"
    fi
done
# exit with an error to leave terminal open
exit 1
