#!/bin/bash

printf "Current directory: %s\n" "$PWD"
printf "Path to this script: %s\n" "$0"
argv=("$@")
argc=${#argv[@]}
for ((i = 0; i < $argc; i++)); do
    arg="${argv[$i]}"
    printf 'Argument #%d: %s\n' "$(($i + 1))" "$arg"
    if [[ -e "$arg" ]]; then
        stat "$arg"
    fi
done
# exit with an error to leave terminal open
exit 1
