#!/bin/sh
set -eu
destination=${1:?license output directory required}
mkdir -p "$destination"
destination=$(cd "$destination" && pwd)
for source in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/* vendor/tsclientlib; do
    test -d "$source" || continue
    package=$(basename "$source")
    mkdir -p "$destination/$package"
    find "$source" -maxdepth 2 -type f \( -iname 'LICENSE*' -o -iname 'NOTICE*' -o -iname 'COPYING*' \) -exec cp '{}' "$destination/$package/" \;
done
for package in react react-dom scheduler; do
    test -f "web/node_modules/$package/LICENSE" || continue
    mkdir -p "$destination/npm-$package"
    cp "web/node_modules/$package/LICENSE" "$destination/npm-$package/"
done
cp LICENSE THIRD_PARTY_NOTICES.md "$destination/"

cp -R licenses/audio "$destination/"
