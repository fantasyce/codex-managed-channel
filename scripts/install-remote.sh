#!/bin/sh
set -eu

bundle=
public_key=
managed_alias=
version=v0.1.0
while [ "$#" -gt 0 ]; do
    case $1 in
        --bundle) bundle=${2-}; shift 2 ;;
        --public-key) public_key=${2-}; shift 2 ;;
        --alias) managed_alias=${2-}; shift 2 ;;
        --version) version=${2-}; shift 2 ;;
        *) printf 'unknown remote-install option\n' >&2; exit 2 ;;
    esac
done
case $managed_alias in ''|*[!A-Za-z0-9._-]*) printf 'invalid managed alias\n' >&2; exit 2 ;; esac
printf '%s\n' "$version" | awk '/^v[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$/ {ok=1} END {exit ok ? 0 : 1}' || {
    printf 'invalid version\n' >&2
    exit 2
}
[ -d "$bundle" ] && [ -f "$bundle/MANIFEST.sha256" ] && [ -f "$public_key" ] || {
    printf 'verified bundle and public key are required\n' >&2
    exit 1
}
for relative in bin/codex-managed-entry bin/codex-managed-preflight scripts/uninstall-remote.sh; do
    [ -f "$bundle/$relative" ] || { printf 'release bundle is incomplete\n' >&2; exit 1; }
done
(cd "$bundle" && shasum -a 256 -c MANIFEST.sha256 >/dev/null) || {
    printf 'release manifest verification failed\n' >&2
    exit 1
}
[ "$(wc -l < "$public_key" | tr -d ' ')" = 1 ] || {
    printf 'public key must contain exactly one line\n' >&2
    exit 1
}
key_line=$(sed -n '1p' "$public_key")
printf '%s\n' "$key_line" | awk '
    NF >= 2 && $1 == "ssh-ed25519" && $2 ~ /^[A-Za-z0-9+\/=]+$/ && length($2) >= 20 { ok = 1 }
    END { exit ok ? 0 : 1 }
' || { printf 'public key is not a valid Ed25519 public key line\n' >&2; exit 1; }

install_root=${CODEX_MANAGED_INSTALL_ROOT:-"$HOME/.local/libexec/codex-managed-channel/$version"}
authorized_keys=${CODEX_MANAGED_AUTHORIZED_KEYS:-"$HOME/.ssh/authorized_keys"}
state_root="$HOME/.local/state/codex-managed-channel/$managed_alias"
case "$install_root:$state_root" in
    *[!A-Za-z0-9_./:-]*) printf 'install path contains unsupported characters\n' >&2; exit 1 ;;
esac

mkdir -p "$install_root/bin" "$install_root/scripts" "$install_root/installs"
chmod 700 "$install_root" "$install_root/bin" "$install_root/scripts" "$install_root/installs"
for relative in bin/codex-managed-entry bin/codex-managed-preflight scripts/uninstall-remote.sh; do
    destination="$install_root/$relative"
    temporary="$destination.$$.new"
    install -m 0755 "$bundle/$relative" "$temporary"
    mv "$temporary" "$destination"
done

if [ "${CODEX_MANAGED_SKIP_PREFLIGHT:-}" != 1 ]; then
    CODEX_MANAGED_ROOT="$state_root" "$install_root/bin/codex-managed-preflight" \
        "printf '%b' '\001\002\003\004\005\006\007\010'; exec codex app-server proxy" \
        >/dev/null
fi

auth_dir=$(dirname "$authorized_keys")
mkdir -p "$auth_dir"
chmod 700 "$auth_dir"
temporary="$auth_dir/.authorized_keys.$$.tmp"
marker="codex-managed-channel:$managed_alias"
if [ -f "$authorized_keys" ]; then
    cp -p "$authorized_keys" "$authorized_keys.bak.$(date +%Y%m%d%H%M%S)"
    awk -v marker="$marker" '$NF != marker { print }' "$authorized_keys" > "$temporary"
else
    : > "$temporary"
fi
printf 'no-port-forwarding,no-X11-forwarding,no-agent-forwarding,no-pty,no-user-rc,command="env CODEX_MANAGED_ROOT=%s %s/bin/codex-managed-entry" %s %s\n' \
    "$state_root" "$install_root" "$key_line" "$marker" >> "$temporary"
chmod 600 "$temporary"
mv "$temporary" "$authorized_keys"
: > "$install_root/installs/$managed_alias"
chmod 600 "$install_root/installs/$managed_alias"
printf 'remote managed entry installed\n'
