#!/usr/bin/env bash
# Instructions and cycles per operation, from `perf stat` on two run lengths of the `work` binary, so the setup cancels out.
# Wall clock times of single microsecond operations are too noisy on this machine but instruction counts are not.
# Usage: ./count.sh <work binary> [cpu]
set -euo pipefail
work=$1
cpu=${2:-2}

counts() { # case what iterations -> "instructions cycles"
    taskset -c "$cpu" perf stat -x, -e instructions:u,cycles:u "$work" "$1" "$2" "$3" 2>&1 >/dev/null \
        | awk -F, '/cpu_core\/instructions/ {i=$1} /cpu_core\/cycles/ {c=$1} END {print i, c}'
}

printf '%-24s %-6s %14s %14s\n' case what instructions cycles
for case in male_body female_body male_body_greyscale female_body_greyscale; do
    for what in parse apply sheet; do
        case $what in sheet) a=10; b=30 ;; *) a=200; b=600 ;; esac
        read -r ia ca <<< "$(counts "$case" "$what" "$a")"
        read -r ib cb <<< "$(counts "$case" "$what" "$b")"
        printf '%-24s %-6s %14d %14d\n' "$case" "$what" $(((ib - ia) / (b - a))) $(((cb - ca) / (b - a)))
    done
done
