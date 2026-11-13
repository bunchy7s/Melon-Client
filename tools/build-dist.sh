#!/bin/sh
# Fat-LTO release into dist/cdn/
#
#   dist/cdn/game/<ver>/<os-arch>/{melon,melon.sha256}
#   dist/cdn/launcher/<ver>/<os-arch>/{melon-launcher,melon-launcher.sha256}
#
# Usage:
#   tools/build-dist.sh
#   tools/build-dist.sh --bin launcher|game
#   tools/build-dist.sh --target linux-x86-64
#   tools/build-dist.sh --list
#   tools/build-dist.sh --quick     incremental thin-LTO build into
#                                   dist/cdn-quick/ (to try, never to ship)
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# CDN folder, rustc triple.
targets() {
    cat <<'EOF'
linux-x86-64        x86_64-unknown-linux-gnu
EOF
}

# Uncomment a line here (or move it into targets) to ship that folder.
later_targets() {
    cat <<'EOF'
# windows-x86-64      x86_64-pc-windows-gnu
# windows-aarch64     aarch64-pc-windows-gnullvm
# macos-x86-64        x86_64-apple-darwin
# macos-aarch64       aarch64-apple-darwin
# linux-riscv64       riscv64gc-unknown-linux-gnu
# linux-aarch64       aarch64-unknown-linux-gnu
# linux-arm           armv7-unknown-linux-gnueabihf
# freebsd-x86-64      x86_64-unknown-freebsd
# freebsd-aarch64     aarch64-unknown-freebsd
# freebsd-riscv64     riscv64gc-unknown-freebsd
EOF
}

what=all
only_folder=
list_only=0
profile=dist
while [ $# -gt 0 ]; do
    case $1 in
        --bin)
            what=${2:?--bin needs launcher or game}
            shift 2
            ;;
        --target)
            only_folder=${2:?--target needs a CDN folder, e.g. linux-x86-64}
            shift 2
            ;;
        --list)
            list_only=1
            shift
            ;;
        --quick)
            profile=dist-quick
            shift
            ;;
        -h|--help)
            cat <<EOF
Fat-LTO release into dist/cdn/, ready to paste onto the CDN.

  dist/cdn/discover.json
  dist/cdn/discover.json.sha256
  dist/cdn/game/<ver>/<os-arch>/melon[.exe]
  dist/cdn/game/<ver>/<os-arch>/melon[.exe].sha256
  dist/cdn/launcher/<ver>/<os-arch>/melon-launcher[.exe]
  dist/cdn/launcher/<ver>/<os-arch>/melon-launcher[.exe].sha256

Ships now:
  linux-x86-64     x86_64-unknown-linux-gnu

Commented:
  # windows-x86-64      x86_64-pc-windows-gnu
  # windows-aarch64     aarch64-pc-windows-gnullvm
  # macos-x86-64        x86_64-apple-darwin
  # macos-aarch64       aarch64-apple-darwin
  # linux-riscv64       riscv64gc-unknown-linux-gnu
  # linux-aarch64       aarch64-unknown-linux-gnu
  # linux-arm           armv7-unknown-linux-gnueabihf
  # freebsd-x86-64      x86_64-unknown-freebsd
  # freebsd-aarch64     aarch64-unknown-freebsd
  # freebsd-riscv64     riscv64gc-unknown-freebsd

Default: fat LTO ([profile.dist]). Incremental rustc cannot run with
fat LTO (rustc turns it off). Unchanged path crates such as Pumpkin
are already skipped by Cargo fingerprints; the remaining cost is the
LTO link of melon.

Usage:
  tools/build-dist.sh
  tools/build-dist.sh --bin launcher|game
  tools/build-dist.sh --target linux-x86-64
  tools/build-dist.sh --list
  tools/build-dist.sh --quick   incremental thin-LTO build into dist/cdn-quick/
                                (fat LTO can't be incremental; use it to try
                                changes, then build the real dist without it)
EOF
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            echo "usage: tools/build-dist.sh [--bin launcher|game] [--target FOLDER] [--quick] [--list]" >&2
            exit 1
            ;;
    esac
done

toml_version() {
    sed -n 's/^version = "\(.*\)"/\1/p' "$1" | head -1
}

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "need $1 on PATH" >&2
        exit 1
    fi
}

need_cmd rustc
need_cmd cargo

rustc_ver=$(rustc --version)
rustc_host=$(rustc -vV | sed -n 's/^host: //p')
want_rust=$(sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml | head -1)
echo "$rustc_ver"
echo "host   $rustc_host"
if [ -n "$want_rust" ]; then
    echo "crate  rust-version $want_rust"
fi
echo "profile $profile"

# Std for $1 is present (rustup target add, or the host toolchain).
has_rustc_target() {
    triple=$1
    libdir=$(rustc --print target-libdir --target "$triple" 2>/dev/null) || return 1
    [ -n "$libdir" ] && [ -d "$libdir" ]
}

exe_suffix() {
    case $1 in
        *windows*) echo .exe ;;
        *) echo ;;
    esac
}

write_sha() {
    dir=$1
    name=$2
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$dir" && sha256sum "$name" > "$name.sha256")
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$dir" && shasum -a 256 "$name" > "$name.sha256")
    else
        echo "need sha256sum or shasum to write $dir/$name.sha256" >&2
        exit 1
    fi
}

publish() {
    src=$1
    dest_dir=$2
    name=$3
    if [ ! -f "$src" ]; then
        echo "missing build output: $src" >&2
        exit 1
    fi
    mkdir -p "$dest_dir"
    rm -f "$dest_dir/$name" "$dest_dir/$name.sha256"
    cp -f "$src" "$dest_dir/$name"
    chmod 755 "$dest_dir/$name" 2>/dev/null || chmod 644 "$dest_dir/$name"
    write_sha "$dest_dir" "$name"
    echo "  $dest_dir/$name"
    echo "  $dest_dir/$name.sha256"
}

game_ver=$(toml_version Cargo.toml)
launcher_ver=$(toml_version ../Melon-Launcher/Cargo.toml)
cdn="$root/dist/cdn"
if [ "$profile" = dist-quick ]; then
    cdn="$root/dist/cdn-quick"
    echo "QUICK build (thin LTO, incremental): not for the CDN"
fi

known_folder() {
    targets | awk '{print $1}' | grep -qx "$1"
}

# Azure Application (client) ID baked into both binaries (login + refresh).
# Public GUID, not a secret. Own app only.
is_ms_guid() {
    echo "$1" | grep -Eqi '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
}

load_ms_client_id() {
    id=${MELON_MS_CLIENT_ID:-}
    if [ -z "$id" ] && [ -f "$root/client_id.txt" ]; then
        id=$(tr -d ' \t\r\n' < "$root/client_id.txt")
    fi
    id=$(printf '%s' "$id" | tr -d ' \t\r\n')
    if [ -z "$id" ]; then
        echo "Microsoft client ID  public default (src/auth PUBLIC_CLIENT_ID)"
        return 0
    fi
    if ! is_ms_guid "$id"; then
        echo "MELON_MS_CLIENT_ID is not a GUID: $id" >&2
        exit 1
    fi
    export MELON_MS_CLIENT_ID=$id
    echo "Microsoft client ID  ${id%%-*}-… (override baked into this dist)"
}

if [ "$list_only" -eq 1 ]; then
    printf '%-18s %-36s %s\n' "folder" "rustc target" "std"
    while read -r folder triple; do
        [ -n "${folder:-}" ] || continue
        if has_rustc_target "$triple"; then
            state=yes
        else
            state="no  (rustup target add $triple)"
        fi
        printf '%-18s %-36s %s\n' "$folder" "$triple" "$state"
    done <<EOF
$(targets)
EOF
    echo
    echo "commented (uncomment in later_targets to ship):"
    later_targets
    exit 0
fi

case $what in
    all|game|melon|launcher|melon-launcher) ;;
    *)
        echo "--bin expects launcher or game, not $what" >&2
        exit 1
        ;;
esac

if [ -n "$only_folder" ] && ! known_folder "$only_folder"; then
    echo "unknown --target $only_folder (see --list)" >&2
    exit 1
fi

load_ms_client_id

build_one() {
    folder=$1
    triple=$2
    ext=$(exe_suffix "$triple")
    out_dir="$root/target/$triple/$profile"

    if ! has_rustc_target "$triple"; then
        echo "skip  $folder  ($triple): rustc has no std — rustup target add $triple"
        return 0
    fi

    echo
    echo "== $folder  ($triple) =="

    if [ "$what" = all ] || [ "$what" = game ] || [ "$what" = melon ]; then
        cargo build --profile "$profile" --target "$triple" --bin melon
        publish "$out_dir/melon$ext" "$cdn/game/$game_ver/$folder" "melon$ext"
    fi
    if [ "$what" = all ] || [ "$what" = launcher ] || [ "$what" = melon-launcher ]; then
        cp "$root/discover.json" ../Melon-Launcher/discover.json
        cargo build --manifest-path ../Melon-Launcher/Cargo.toml --target-dir "$root/target" --profile "$profile" --target "$triple"
        publish "$out_dir/melon-launcher$ext" "$cdn/launcher/$launcher_ver/$folder" "melon-launcher$ext"
    fi
}

while read -r folder triple; do
    [ -n "${folder:-}" ] || continue
    if [ -n "$only_folder" ] && [ "$folder" != "$only_folder" ]; then
        continue
    fi
    build_one "$folder" "$triple"
done <<EOF
$(targets)
EOF

write_discover() {
    if ! command -v python3 >/dev/null 2>&1; then
        echo "need python3 to write dist/cdn/discover.json" >&2
        exit 1
    fi
    python3 - "$root" "$cdn" "$game_ver" "$launcher_ver" <<'PY'
import json, sys
from pathlib import Path

root, cdn, game_ver, launcher_ver = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3], sys.argv[4]
src = root / "discover.json"
data = json.loads(src.read_text()) if src.is_file() else {}
latest = data.setdefault("latest", {})
latest["game"] = game_ver
latest["launcher"] = launcher_ver
data["latest"] = latest

def ensure(kind, version):
    items = data.setdefault(kind, [])
    if not any(item.get("version") == version for item in items):
        items.append({"version": version, "platforms": {}})
    data[kind] = items

ensure("game", game_ver)
ensure("launcher", launcher_ver)

def ingest(kind, names):
    kind_dir = cdn / kind
    if not kind_dir.is_dir():
        return
    by_ver = {item.get("version"): item for item in data.get(kind, [])}
    for ver_dir in sorted(p for p in kind_dir.iterdir() if p.is_dir()):
        version = ver_dir.name
        item = by_ver.setdefault(version, {"version": version, "platforms": {}})
        platforms = item.setdefault("platforms", {})
        for plat_dir in sorted(p for p in ver_dir.iterdir() if p.is_dir()):
            for name in names:
                sha = plat_dir / f"{name}.sha256"
                if not sha.is_file():
                    continue
                token = sha.read_text().split()
                if token:
                    platforms[plat_dir.name] = {"sha256": token[0].lower()}
                break
        item["platforms"] = platforms
        by_ver[version] = item
    data[kind] = list(by_ver.values())

ingest("game", ["melon", "melon.exe"])
ingest("launcher", ["melon-launcher", "melon-launcher.exe"])

cdn.mkdir(parents=True, exist_ok=True)
out = cdn / "discover.json"
out.write_text(json.dumps(data, indent=2) + "\n")
print(out)
PY
    write_sha "$cdn" "discover.json"
    echo "  $cdn/discover.json"
    echo "  $cdn/discover.json.sha256"
}

write_discover

echo
echo "Paste into melonmc/cdn/:"
echo "  $cdn/discover.json  ->  cdn/discover.json"
echo "  $cdn/discover.json.sha256  ->  cdn/discover.json.sha256"
if [ -d "$cdn/game/$game_ver" ]; then
    find "$cdn/game/$game_ver" -mindepth 1 -maxdepth 1 -type d | sort | while read -r d; do
        echo "  $d  ->  cdn/game/$game_ver/$(basename "$d")/"
    done
fi
if [ -d "$cdn/launcher/$launcher_ver" ]; then
    find "$cdn/launcher/$launcher_ver" -mindepth 1 -maxdepth 1 -type d | sort | while read -r d; do
        echo "  $d  ->  cdn/launcher/$launcher_ver/$(basename "$d")/"
    done
fi
