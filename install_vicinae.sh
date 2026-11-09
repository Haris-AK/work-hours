#!/bin/sh
# Build and install the interactive Vicinae extension into Vicinae's data directory.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ ! -x "$HOME/.local/lib/work-hours/work-hours" ]; then
    printf 'Install the Rust tracker first: ./install.sh\n' >&2
    exit 1
fi
npm ci --prefix "$repo_dir/vicinae"
npm run --prefix "$repo_dir/vicinae" build
printf 'In Vicinae, search for Work Hours — Manage Timer and History.\n'
