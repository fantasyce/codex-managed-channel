#!/bin/sh
set -eu

usage() {
    cat <<'EOF'
Usage: uninstall.sh --remote ADMIN_ALIAS --alias MANAGED_ALIAS [--purge purge]

Removes only the selected managed alias and remote authorization. Runtime logs
and state are retained unless --purge purge is supplied.
EOF
}

remote=
managed_alias=
purge=
while [ "$#" -gt 0 ]; do
    case $1 in
        --remote) remote=${2-}; shift 2 ;;
        --alias) managed_alias=${2-}; shift 2 ;;
        --purge) purge=${2-}; shift 2 ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'unknown option\n' >&2; exit 2 ;;
    esac
done
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$script_dir/scripts/install-lib.sh"
validate_alias "$remote" || { printf 'invalid administrative alias\n' >&2; exit 2; }
validate_alias "$managed_alias" || { printf 'invalid managed alias\n' >&2; exit 2; }
if [ -n "$purge" ] && [ "$purge" != purge ]; then
    printf 'purge requires the literal confirmation: purge\n' >&2
    exit 2
fi
ssh -o BatchMode=yes "$remote" true >/dev/null 2>&1 || {
    printf 'administrative SSH probe failed\n' >&2
    exit 1
}
remote_command='"$HOME/.local/libexec/codex-managed-channel/scripts/uninstall-remote.sh" --alias '"'$managed_alias'"
if [ "$purge" = purge ]; then
    remote_command="$remote_command --purge purge"
fi
ssh "$remote" "/bin/sh -c '$remote_command'"

ssh_config="$HOME/.ssh/config"
if [ -f "$ssh_config" ]; then
    cp -p "$ssh_config" "$ssh_config.bak.$(date +%Y%m%d%H%M%S)"
    remove_managed_config "$ssh_config" "$managed_alias"
fi
key_path="$HOME/.ssh/codex-managed-channel/$managed_alias"
if [ -f "$key_path" ] && [ -f "$key_path.pub" ] && grep -q "codex-managed-channel:$managed_alias" "$key_path.pub"; then
    trash="$HOME/.Trash/codex-managed-channel-$managed_alias-$(date +%Y%m%d%H%M%S)"
    mkdir -p "$trash"
    mv "$key_path" "$key_path.pub" "$trash/"
fi
printf 'Removed managed SSH alias: %s\n' "$managed_alias"
