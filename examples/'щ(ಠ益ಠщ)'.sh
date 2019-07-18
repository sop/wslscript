#!/bin/bash
# This script has unicode characters and single quotes in its filename.

printf 'This script'\''s filename is "%s"\n' "$0"
printf 'Canonical path is "%s"\n' "$(readlink -f "$0")"
stat "$0"
exit 1
