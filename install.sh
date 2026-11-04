#!/usr/bin/env bash
# Installs (or, with --uninstall, removes) scd from the statically-linked
# (musl) release tarball, placing files in the same locations the .deb/.rpm
# packages use. __SC_VERSION__ is substituted with the release tag by the
# packages.yml workflow before this script is uploaded as a release asset.
set -euo pipefail

# Everything lives in main() so a truncated `curl | bash` (network cut off
# mid-download) fails to parse instead of executing a partial script.
main() {
    VERSION="__SC_VERSION__"
    REPO="aranor01/sc"
    TARGET="x86_64-unknown-linux-musl"
    PREFIX="${SC_INSTALL_PREFIX:-/usr}"

    sudo=""
    if [ "$(id -u)" -ne 0 ]; then
        if command -v sudo >/dev/null 2>&1; then
            sudo="sudo"
        else
            echo "error: must be run as root, or have sudo available, for $PREFIX" >&2
            exit 1
        fi
    fi

    # Removes the 0.1.0 install (binaries `sc`/`sc-action`, dirs `share/sc`,
    # `share/doc/sc`). `$PREFIX/bin/sc` alone could be an unrelated program
    # (the spreadsheet `sc`), so only act if our scripts dir is present.
    remove_legacy() {
        if [ -d "$PREFIX/share/sc/scripts" ]; then
            $sudo rm -f "$PREFIX/bin/sc" "$PREFIX/bin/sc-action"
            $sudo rm -rf "$PREFIX/share/sc" "$PREFIX/share/doc/sc"
            echo "Removed the previous 'sc' install from $PREFIX"
        fi
    }

    if [ "${1:-}" = "--uninstall" ]; then
        remove_legacy
        $sudo rm -f "$PREFIX/bin/scd" "$PREFIX/bin/scd-action"
        $sudo rm -rf "$PREFIX/share/scd/scripts" "$PREFIX/share/doc/scd"
        $sudo rmdir "$PREFIX/share/scd" 2>/dev/null || true
        echo "Uninstalled scd from $PREFIX"
        exit 0
    fi

    os="$(uname -s)"
    arch="$(uname -m)"
    if [ "$os" != "Linux" ] || [ "$arch" != "x86_64" ]; then
        echo "error: unsupported platform ($os/$arch) — scd only ships prebuilt x86_64 Linux binaries" >&2
        exit 1
    fi

    for dep in curl tar install sha256sum; do
        if ! command -v "$dep" >/dev/null 2>&1; then
            echo "error: '$dep' is required but not installed" >&2
            exit 1
        fi
    done

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    tarball="scd-$TARGET.tar.gz"
    url="https://github.com/$REPO/releases/download/$VERSION/$tarball"
    echo "Downloading $url"
    curl -fsSL "$url" -o "$tmpdir/$tarball"
    curl -fsSL "$url.sha256" -o "$tmpdir/$tarball.sha256"
    echo "Verifying checksum"
    (cd "$tmpdir" && sha256sum -c "$tarball.sha256")
    tar xzf "$tmpdir/$tarball" -C "$tmpdir"

    remove_legacy

    bundle="$tmpdir/scd-$TARGET"
    $sudo install -Dm755 "$bundle/scd" "$PREFIX/bin/scd"
    $sudo install -Dm755 "$bundle/scd-action" "$PREFIX/bin/scd-action"
    for f in "$bundle"/scripts/*.sh; do
        $sudo install -Dm755 "$f" "$PREFIX/share/scd/scripts/$(basename "$f")"
    done
    for f in "$bundle/README.md" "$bundle"/docs/*.md; do
        $sudo install -Dm644 "$f" "$PREFIX/share/doc/scd/$(basename "$f")"
    done

    echo "Installed scd $VERSION to $PREFIX"
}

main "$@"
