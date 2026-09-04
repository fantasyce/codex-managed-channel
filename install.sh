#!/bin/sh
set -eu

usage() {
    cat <<'EOF'
Usage: install.sh --remote ADMIN_ALIAS --alias MANAGED_ALIAS [options]

Options:
  --remote ALIAS       Existing administrative SSH alias
  --alias ALIAS        New managed SSH alias
  --version VERSION    Release tag (default: v0.1.0)
  --repository OWNER/REPOSITORY
                       GitHub repository override
  --key PATH           Reuse an existing dedicated private key
  --help               Show this help
EOF
}

remote=
managed_alias=
version=v0.1.0
repository=${CODEX_MANAGED_REPOSITORY:-example/codex-managed-channel}
key_path=
while [ "$#" -gt 0 ]; do
    case $1 in
        --remote|--alias|--version|--repository|--key)
            [ "$#" -ge 2 ] || { printf 'missing value for %s\n' "$1" >&2; exit 2; }
            option=$1
            value=$2
            shift 2
            case $option in
                --remote) remote=$value ;;
                --alias) managed_alias=$value ;;
                --version) version=$value ;;
                --repository) repository=$value ;;
                --key) key_path=$value ;;
            esac
            ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" 2>/dev/null && pwd || true)
library="$script_dir/scripts/install-lib.sh"
bootstrap_dir=
if [ ! -f "$library" ]; then
    command -v curl >/dev/null 2>&1 || { printf 'required command is unavailable: curl\n' >&2; exit 1; }
    bootstrap_dir=$(mktemp -d -t codex-managed-bootstrap)
    library="$bootstrap_dir/install-lib.sh"
    curl -fsSL "https://raw.githubusercontent.com/$repository/$version/scripts/install-lib.sh" -o "$library"
fi
. "$library"
work_dir=
cleanup() {
    delete_temporary_tree "$work_dir" || true
    delete_temporary_tree "$bootstrap_dir" || true
}
trap cleanup EXIT HUP INT TERM

validate_alias "$remote" || { printf 'invalid administrative alias\n' >&2; exit 2; }
validate_alias "$managed_alias" || { printf 'invalid managed alias\n' >&2; exit 2; }
[ "$remote" != "$managed_alias" ] || { printf 'administrative and managed aliases must differ\n' >&2; exit 2; }
case $repository in */*) ;; *) printf 'invalid repository\n' >&2; exit 2 ;; esac
case $version in v[0-9]*.[0-9]*.[0-9]*) ;; *) printf 'invalid version\n' >&2; exit 2 ;; esac

ssh_dir=${CODEX_MANAGED_SSH_DIR:-"$HOME/.ssh"}
ssh_config=${CODEX_MANAGED_SSH_CONFIG:-"$ssh_dir/config"}
config_has_unmanaged_alias "$ssh_config" "$managed_alias" && {
    printf 'managed alias already exists outside this installer block\n' >&2
    exit 1
}

for command_name in ssh scp ssh-keygen curl shasum tar mktemp awk install uname; do
    require_command "$command_name"
done
[ "$(uname -s)" = Darwin ] || { printf 'the client must run macOS\n' >&2; exit 1; }
resolved=$(ssh -G "$remote" 2>/dev/null) || { printf 'unable to resolve administrative SSH alias\n' >&2; exit 1; }
host_name=$(printf '%s\n' "$resolved" | awk '$1 == "hostname" {print $2; exit}')
user_name=$(printf '%s\n' "$resolved" | awk '$1 == "user" {print $2; exit}')
port=$(printf '%s\n' "$resolved" | awk '$1 == "port" {print $2; exit}')
proxy_jump=$(printf '%s\n' "$resolved" | awk '$1 == "proxyjump" {print $2; exit}')
proxy_command=$(printf '%s\n' "$resolved" | awk '$1 == "proxycommand" {$1=""; sub(/^ /, ""); print; exit}')
[ -n "$host_name" ] && [ -n "$user_name" ] && [ -n "$port" ] || {
    printf 'administrative SSH alias is incomplete\n' >&2
    exit 1
}
[ -z "$proxy_command" ] || [ "$proxy_command" = none ] || {
    printf 'ProxyCommand is outside the version 0.1 support boundary\n' >&2
    exit 1
}
ssh -o BatchMode=yes "$remote" true >/dev/null 2>&1 || {
    printf 'administrative SSH probe failed\n' >&2
    exit 1
}
remote_platform=$(ssh -o BatchMode=yes "$remote" 'printf "%s|%s\n" "$(uname -s)" "$(uname -m)"')
case $remote_platform in
    Darwin\|arm64) target=aarch64-apple-darwin ;;
    Darwin\|x86_64) target=x86_64-apple-darwin ;;
    *) printf 'the remote host must run supported macOS\n' >&2; exit 1 ;;
esac

work_dir=$(mktemp -d -t codex-managed-install)
archive_name="codex-managed-channel-${version}-${target}.tar.gz"
base_url=${CODEX_MANAGED_RELEASE_BASE_URL:-"https://github.com/$repository/releases/download/$version"}
curl -fL "$base_url/$archive_name" -o "$work_dir/$archive_name"
curl -fL "$base_url/SHA256SUMS" -o "$work_dir/SHA256SUMS"
verify_checksum "$work_dir/$archive_name" "$work_dir/SHA256SUMS"

if [ -z "$key_path" ]; then
    key_dir="$ssh_dir/codex-managed-channel"
    key_path="$key_dir/$managed_alias"
    mkdir -p "$key_dir"
    chmod 700 "$key_dir"
    if [ ! -f "$key_path" ]; then
        ssh-keygen -q -t ed25519 -N '' -C "codex-managed-channel:$managed_alias" -f "$key_path"
    fi
else
    [ -f "$key_path" ] && [ -f "$key_path.pub" ] || {
        printf 'the supplied key and public key are required\n' >&2
        exit 1
    }
fi
chmod 600 "$key_path"

remote_stage=$(ssh "$remote" 'mktemp -d -t codex-managed-install')
case $remote_stage in */codex-managed-install.*) ;; *) printf 'remote temporary path is invalid\n' >&2; exit 1 ;; esac
scp -q "$work_dir/$archive_name" "$key_path.pub" "$remote:$remote_stage/"
public_name=$(basename "$key_path.pub")
ssh "$remote" "/bin/sh -s -- '$remote_stage' '$archive_name' '$managed_alias' '$public_name' '$version'" <<'REMOTE'
set -eu
stage=$1
archive=$2
managed_alias=$3
public_name=$4
version=$5
tar -xzf "$stage/$archive" -C "$stage"
/bin/sh "$stage/codex-managed-channel/scripts/install-remote.sh" \
  --bundle "$stage/codex-managed-channel" \
  --public-key "$stage/$public_name" \
  --alias "$managed_alias"
  --version "$version"
REMOTE
ssh "$remote" "find '$remote_stage' -depth -delete"

mkdir -p "$ssh_dir"
chmod 700 "$ssh_dir"
if [ -f "$ssh_config" ]; then
    cp -p "$ssh_config" "$ssh_config.bak.$(date +%Y%m%d%H%M%S)"
fi
write_managed_config "$ssh_config" "$managed_alias" "$host_name" "$user_name" "$port" "$key_path" "${proxy_jump:-none}"
ssh -F "$ssh_config" -o BatchMode=yes "$managed_alias" 'codex --version' >/dev/null
printf 'Installed managed SSH alias: %s\n' "$managed_alias"
