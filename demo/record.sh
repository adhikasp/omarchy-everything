#!/bin/bash
# Regenerate the README media (docs/media/*) from fictional demo data.
#
#   demo/record.sh            # stills + demo.gif into docs/media/
#
# The plugin is copied to a temp dir and pointed at fakes: demo/helper.py
# serves demo/items.json, HOME-derived paths go to a fake home holding the
# demo clipboard and rates, and demo/fake-bin shadows `omarchy` and
# `timedatectl`. The stage grabs only its own card item, so the real desktop
# never appears. It does briefly open the panel on screen while recording.
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
OMARCHY_SOURCE=${OMARCHY_PATH:-/usr/share/omarchy}
OUT=${DEMO_MEDIA_DIR:-$ROOT/docs/media}
TMP=$(mktemp -d)
trap 'rm -rf -- "$TMP"' EXIT

PLUGIN="$TMP/plugin"
HOME_FAKE="$TMP/home"
mkdir -p "$PLUGIN/rust/target/release" "$HOME_FAKE/.local/state/omarchy" "$HOME_FAKE/.cache" \
  "$TMP/config" "$TMP/frames" "$TMP/bin"

cp -- "$ROOT"/{Service.qml,Everything.qml,CenteredPanel.qml,EverythingModel.js,InstantAnswers.js} "$PLUGIN/"
install -m 755 -- "$ROOT/demo/helper.py" "$PLUGIN/rust/target/release/everything-helper"
install -m 755 -- "$ROOT"/demo/fake-bin/* "$TMP/bin/"
cp -- "$ROOT/demo/clipboard-history.json" "$HOME_FAKE/.local/state/omarchy/"
python3 - "$ROOT/demo/rates.json" "$HOME_FAKE/.cache/omarchy-everything-rates.json" <<'PY'
import json, sys, time
rates = json.load(open(sys.argv[1]))
rates["fetchedAt"] = int(time.time() * 1000)
open(sys.argv[2], "w").write(json.dumps(rates))
PY

# The copy, not the source: its own shortcut id and the fake home.
sed -i 's/appid: "omarchy-everything"/appid: "omarchy-everything-demo"/' "$PLUGIN/Everything.qml"
sed -i "s|Quickshell.env(\"HOME\")|\"$HOME_FAKE\"|g" "$PLUGIN/Service.qml"

cp -- "$ROOT/demo/shell.qml" "$TMP/config/shell.qml"
# Copies, not symlinks: quickshell ignores qmldir (singletons such as the
# theme's Color) for module paths outside the config folder.
cp -a -- "$OMARCHY_SOURCE/shell/Commons" "$TMP/config/Commons"
cp -a -- "$OMARCHY_SOURCE/shell/Ui" "$TMP/config/Ui"

env PATH="$TMP/bin:$PATH" \
  EVERYTHING_SOURCE_DIR="$PLUGIN" \
  EVERYTHING_DEMO_ITEMS="$ROOT/demo/items.json" \
  DEMO_OUT="$TMP/frames" \
  OMARCHY_PATH="$OMARCHY_SOURCE" \
  timeout 120 quickshell -p "$TMP/config" --no-color >"$TMP/stage.log" 2>&1 || true

if ! grep -q DEMO_DONE "$TMP/stage.log"; then
  tail -n 25 "$TMP/stage.log" >&2
  exit 1
fi
grep -E "DEMO_COLORS" "$TMP/stage.log" >&2 || true
[[ -n ${DEMO_KEEP:-} ]] && cp -r "$TMP/frames" "$DEMO_KEEP"
grep -o 'DEMO_FRAME .*' "$TMP/stage.log" | sed 's/^DEMO_FRAME //' >"$TMP/frames.jsonl"

python3 "$ROOT/demo/compose.py" "$TMP/frames" "$TMP/frames.jsonl" "$OUT"
