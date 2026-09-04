#!/bin/sh

validate_alias() {
    case ${1-} in
        ''|*[!A-Za-z0-9._-]*) return 1 ;;
        *) return 0 ;;
    esac
}

validate_version() {
    printf '%s\n' "${1-}" | awk '
        /^v[0-9]+\.[0-9]+\.[0-9]+([.-][A-Za-z0-9.-]+)?$/ { ok = 1 }
        END { exit ok ? 0 : 1 }
    '
}

validate_repository() {
    printf '%s\n' "${1-}" | awk '
        /^[A-Za-z0-9._-]+\/[A-Za-z0-9._-]+$/ { ok = 1 }
        END { exit ok ? 0 : 1 }
    '
}

verify_archive_paths() {
    archive=$1
    tar -tzf "$archive" | awk '
        /^\// || /(^|\/)\.\.?(\/|$)/ { bad = 1 }
        $0 !~ /^codex-managed-channel\// { bad = 1 }
        END { exit bad ? 1 : 0 }
    '
}

require_command() {
    command -v "$1" >/dev/null 2>&1 || {
        printf 'required command is unavailable: %s\n' "$1" >&2
        return 1
    }
}

verify_checksum() {
    artifact=$1
    sums=$2
    name=$(basename "$artifact")
    expected=$(awk -v name="$name" '$2 == name || $2 == "*" name { print $1 }' "$sums")
    [ -n "$expected" ] || {
        printf 'checksum entry is missing\n' >&2
        return 1
    }
    [ "$(printf '%s\n' "$expected" | wc -l | tr -d ' ')" = 1 ] || {
        printf 'checksum entry is ambiguous\n' >&2
        return 1
    }
    actual=$(shasum -a 256 "$artifact" | awk '{print $1}')
    [ "$actual" = "$expected" ] || {
        printf 'checksum verification failed\n' >&2
        return 1
    }
}

config_has_unmanaged_alias() {
    config=$1
    alias_name=$2
    [ -f "$config" ] || return 1
    awk -v alias_name="$alias_name" '
        $0 == "# BEGIN codex-managed-channel " alias_name { managed = 1; next }
        $0 == "# END codex-managed-channel " alias_name { managed = 0; next }
        !managed && $1 == "Host" {
            for (i = 2; i <= NF; i++) if ($i == alias_name) found = 1
        }
        END { exit found ? 0 : 1 }
    ' "$config"
}

write_managed_config() {
    config=$1
    alias_name=$2
    host_name=$3
    user_name=$4
    port=$5
    identity_file=$6
    proxy_jump=$7
    host_key_alias=${8-}
    user_known_hosts=${9-}
    strict_host_key=${10-}
    check_host_ip=${11-}
    begin="# BEGIN codex-managed-channel $alias_name"
    end="# END codex-managed-channel $alias_name"
    directory=$(dirname "$config")
    mkdir -p "$directory"
    temporary="$directory/.config.$$.tmp"
    if [ -f "$config" ]; then
        awk -v begin="$begin" -v end="$end" '
            $0 == begin { skip = 1; next }
            $0 == end { skip = 0; next }
            !skip { print }
        ' "$config" > "$temporary"
    else
        : > "$temporary"
    fi
    while [ -s "$temporary" ] && [ "$(tail -c 1 "$temporary" | wc -l | tr -d ' ')" = 0 ]; do
        printf '\n' >> "$temporary"
    done
    {
        printf '%s\n' "$begin"
        printf 'Host %s\n' "$alias_name"
        printf '  HostName %s\n' "$host_name"
        printf '  User %s\n' "$user_name"
        printf '  Port %s\n' "$port"
        printf '  IdentityFile %s\n' "$identity_file"
        printf '  IdentitiesOnly yes\n'
        if [ "$proxy_jump" != none ] && [ -n "$proxy_jump" ]; then
            printf '  ProxyJump %s\n' "$proxy_jump"
        fi
        if [ -n "$host_key_alias" ] && [ "$host_key_alias" != none ]; then
            printf '  HostKeyAlias %s\n' "$host_key_alias"
        fi
        if [ -n "$user_known_hosts" ] && [ "$user_known_hosts" != none ]; then
            printf '  UserKnownHostsFile %s\n' "$user_known_hosts"
        fi
        if [ -n "$strict_host_key" ]; then
            printf '  StrictHostKeyChecking %s\n' "$strict_host_key"
        fi
        if [ -n "$check_host_ip" ]; then
            printf '  CheckHostIP %s\n' "$check_host_ip"
        fi
        printf '%s\n' "$end"
    } >> "$temporary"
    chmod 600 "$temporary"
    mv "$temporary" "$config"
}

remove_managed_config() {
    config=$1
    alias_name=$2
    [ -f "$config" ] || return 0
    directory=$(dirname "$config")
    temporary="$directory/.config.$$.tmp"
    begin="# BEGIN codex-managed-channel $alias_name"
    end="# END codex-managed-channel $alias_name"
    awk -v begin="$begin" -v end="$end" '
        $0 == begin { skip = 1; next }
        $0 == end { skip = 0; next }
        !skip { print }
    ' "$config" > "$temporary"
    chmod 600 "$temporary"
    mv "$temporary" "$config"
}

delete_temporary_tree() {
    path=${1-}
    [ -n "$path" ] && [ -d "$path" ] || return 0
    case $path in
        */codex-managed-bootstrap.*|*/codex-managed-install.*)
            find "$path" -depth -delete
            ;;
        *) printf 'refusing to clean an unexpected temporary path\n' >&2; return 1 ;;
    esac
}
