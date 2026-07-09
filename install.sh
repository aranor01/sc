#!/usr/bin/env bash
# Installs (or, with --uninstall, removes) sc from the statically-linked
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

    if [ "${1:-}" = "--uninstall" ]; then
        $sudo rm -f "$PREFIX/bin/sc" "$PREFIX/bin/sc-action"
        $sudo rm -rf "$PREFIX/share/sc/scripts" "$PREFIX/share/doc/sc"
        $sudo rmdir "$PREFIX/share/sc" 2>/dev/null || true
        echo "Uninstalled sc from $PREFIX"
        exit 0
    fi

    os="$(uname -s)"
    arch="$(uname -m)"
    if [ "$os" != "Linux" ] || [ "$arch" != "x86_64" ]; then
        echo "error: unsupported platform ($os/$arch) — sc only ships prebuilt x86_64 Linux binaries" >&2
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

    tarball="sc-$TARGET.tar.gz"
    url="https://github.com/$REPO/releases/download/$VERSION/$tarball"
    echo "Downloading $url"
    curl -fsSL "$url" -o "$tmpdir/$tarball"
    curl -fsSL "$url.sha256" -o "$tmpdir/$tarball.sha256"
    echo "Verifying checksum"
    (cd "$tmpdir" && sha256sum -c "$tarball.sha256")
    tar xzf "$tmpdir/$tarball" -C "$tmpdir"

    bundle="$tmpdir/sc-$TARGET"
    $sudo install -Dm755 "$bundle/sc" "$PREFIX/bin/sc"
    $sudo install -Dm755 "$bundle/sc-action" "$PREFIX/bin/sc-action"
    for f in "$bundle"/scripts/*.sh; do
        $sudo install -Dm755 "$f" "$PREFIX/share/sc/scripts/$(basename "$f")"
    done
    for f in "$bundle/README.md" "$bundle"/docs/*.md; do
        $sudo install -Dm644 "$f" "$PREFIX/share/doc/sc/$(basename "$f")"
    done

    echo "Installed sc $VERSION to $PREFIX"
}

main "$@"
