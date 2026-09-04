#!/bin/sh
set -eu

managed_alias=
purge=
while [ "$#" -gt 0 ]; do
    case $1 in
        --alias) managed_alias=${2-}; shift 2 ;;
        --purge) purge=${2-}; shift 2 ;;
        *) printf 'unknown remote-uninstall option\n' >&2; exit 2 ;;
    esac
done
case $managed_alias in ''|*[!A-Za-z0-9._-]*) printf 'invalid managed alias\n' >&2; exit 2 ;; esac
if [ -n "$purge" ] && [ "$purge" != purge ]; then
    printf 'purge requires the literal confirmation: purge\n' >&2
    exit 2
fi

install_root=${CODEX_MANAGED_INSTALL_ROOT:-"$HOME/.local/libexec/codex-managed-channel"}
authorized_keys=${CODEX_MANAGED_AUTHORIZED_KEYS:-"$HOME/.ssh/authorized_keys"}
marker="codex-managed-channel:$managed_alias"
if [ -f "$authorized_keys" ]; then
    auth_dir=$(dirname "$authorized_keys")
    temporary="$auth_dir/.authorized_keys.$$.tmp"
    cp -p "$authorized_keys" "$authorized_keys.bak.$(date +%Y%m%d%H%M%S)"
    awk -v marker="$marker" '$NF != marker { print }' "$authorized_keys" > "$temporary"
    chmod 600 "$temporary"
    mv "$temporary" "$authorized_keys"
fi

entry="$install_root/bin/codex-managed-entry"
if [ -x "$entry" ]; then
    ps -axo pid=,command= | awk -v entry="$entry" '$2 == entry { print $1 }' | while IFS= read -r pid; do
        [ -n "$pid" ] && kill -TERM "$pid" 2>/dev/null || true
    done
fi
if [ -f "$install_root/installs/$managed_alias" ]; then
    unlink "$install_root/installs/$managed_alias"
fi
if [ "$purge" = purge ]; then
    state_root="$HOME/.codex-managed"
    if [ -d "$state_root" ]; then
        find "$state_root" -depth -delete
    fi
fi
printf 'remote managed entry removed\n'
