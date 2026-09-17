#!/bin/sh
set -eu

version=${1-}
target=${2-}
case $version in v[0-9]*.[0-9]*.[0-9]*) ;; *) printf 'usage: package-release.sh vX.Y.Z TARGET\n' >&2; exit 2 ;; esac
if [ -z "$target" ]; then
    case $(uname -m) in
        arm64) target=aarch64-apple-darwin ;;
        x86_64) target=x86_64-apple-darwin ;;
        *) printf 'unsupported build architecture\n' >&2; exit 1 ;;
    esac
fi
case $target in aarch64-apple-darwin|x86_64-apple-darwin) ;; *) printf 'unsupported target\n' >&2; exit 1 ;; esac

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dist="$project_dir/dist"
work=$(mktemp -d -t codex-managed-package)
cleanup() {
    if [ -d "$work" ]; then find "$work" -depth -delete; fi
}
trap cleanup EXIT HUP INT TERM
root="$work/codex-managed-channel"
mkdir -p "$root/bin" "$root/scripts" "$dist"

cargo build --locked --release --target "$target" --manifest-path "$project_dir/Cargo.toml"
cargo metadata --locked --format-version 1 --filter-platform "$target" \
    --manifest-path "$project_dir/Cargo.toml" > "$work/cargo-metadata.json"
install -m 0755 "$project_dir/target/$target/release/codex-managed-entry" "$root/bin/"
install -m 0755 "$project_dir/target/$target/release/codex-managed-preflight" "$root/bin/"
install -m 0755 "$project_dir/scripts/install-remote.sh" "$root/scripts/"
install -m 0755 "$project_dir/scripts/uninstall-remote.sh" "$root/scripts/"
install -m 0644 "$project_dir/LICENSE" "$root/LICENSE"
python3 "$project_dir/tools/bundle-third-party-licenses.py" "$work/cargo-metadata.json" "$root"
(
    cd "$root"
    find . -type f ! -name MANIFEST.sha256 -print | LC_ALL=C sort | while IFS= read -r relative; do
        relative=${relative#./}
        shasum -a 256 "$relative"
    done > MANIFEST.sha256
)
archive="codex-managed-channel-${version}-${target}.tar.gz"
COPYFILE_DISABLE=1 tar -czf "$dist/$archive" -C "$work" codex-managed-channel
shasum -a 256 "$dist/$archive" | awk -v name="$archive" '{print $1 "  " name}' > "$dist/SHA256SUMS"
"$project_dir/scripts/privacy-scan.sh" --root "$project_dir" --archive "$dist/$archive"
printf '%s\n' "$dist/$archive"
