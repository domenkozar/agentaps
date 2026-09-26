#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
trunk build index.html --release --public-url /connect/ --dist dist/connect
cp -R site/. dist/
mkdir -p dist/fonts
cp fonts/IBMPlexSans-Regular.ttf dist/fonts/
cp fonts/LICENSE.txt dist/fonts/
