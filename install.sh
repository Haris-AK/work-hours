#!/bin/sh
# Build the Rust binaries, install them outside the repo, and link the entry points.
set -eu
repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
binary_dir="$HOME/.local/lib/work-hours"
bin_link="$HOME/.local/bin/work-hours"
desktop_link="$HOME/.local/share/applications/work-hours.desktop"

check_link() {
    target=$1 source=$2
    if [ -e "$target" ] || [ -L "$target" ]; then
        if [ "$(realpath -m "$target")" != "$(realpath -m "$source")" ]; then
            printf 'Refusing to replace %s\n' "$target" >&2
            exit 1
        fi
    fi
}
link_file() {
    target=$1 source=$2
    mkdir -p "$(dirname -- "$target")"
    if [ ! -e "$target" ] && [ ! -L "$target" ]; then
        ln -s "$source" "$target"
    fi
}
check_link "$bin_link" "$binary_dir/work-hours"
check_link "$desktop_link" "$repo_dir/work-hours.desktop"
cargo build --manifest-path "$repo_dir/cli/Cargo.toml" --release --locked --features dashboard
mkdir -p "$binary_dir"
# Rename each completed copy atomically, allowing upgrades while a binary is running.
for name in work-hours-dashboard work-hours; do
    temporary=$(mktemp "$binary_dir/.$name.XXXXXX")
    install -m 755 "$repo_dir/cli/target/release/$name" "$temporary"
    mv -f "$temporary" "$binary_dir/$name"
done
link_file "$bin_link" "$binary_dir/work-hours"
link_file "$desktop_link" "$repo_dir/work-hours.desktop"
printf 'Work Hours installed (Rust). Search for Work Hours in Vicinae.\n'
if command -v vicinae >/dev/null 2>&1; then
    vicinae cmd launch core:refresh-apps
fi

if command -v vicinae >/dev/null 2>&1 && command -v npm >/dev/null 2>&1; then
    "$repo_dir/install_vicinae.sh"
fi
