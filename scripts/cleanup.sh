#!/usr/bin/env bash
# Confidant — remove everything this build added to a machine, in one command.
#
# Usage:
#   scripts/cleanup.sh            # show what would be removed, with sizes (dry run)
#   scripts/cleanup.sh --yes      # actually remove it
#   scripts/cleanup.sh --yes --keep-repo    # remove runtime/dev state, keep the repo
#
# Nothing here touches the repo's source files unless --keep-repo is absent, in which case
# the *build outputs* (target/, node_modules/, .next/, out/) are removed but sources stay.

set -uo pipefail

YES=0
KEEP_REPO=1
for arg in "$@"; do
  case "$arg" in
    --yes) YES=1 ;;
    --all) KEEP_REPO=0 ;;
  esac
done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOME_DIR="${HOME:-/root}"

PATHS=(
  "$HOME_DIR/.local/opt/bin/ollama"          # user-space ollama binary
  "$HOME_DIR/.local/opt/lib"                 # ollama libs (llama.cpp runner, cuda/rocm stubs)
  "$HOME_DIR/.local/opt/ollama-serve.log"
  "$HOME_DIR/.local/bin/ollama"              # symlink
  "$HOME_DIR/models"                         # model blobs (OLLAMA_MODELS)
  "$HOME_DIR/.ollama"                        # default model dir, if ever used
  "$HOME_DIR/.local/share/confidant"         # the app's SQLite store
  "$HOME_DIR/.config/hf26"                   # the dev.to key + draft id (rotate the key too)
  "/tmp/confidant-audio"                     # transient audio (deleted per-request already)
  "$HOME_DIR/hackathon/confidant/.venv"      # speech-to-text venv
  "$HOME_DIR/hackathon/confidant/scratch"
  "$HOME_DIR/.cache/huggingface"             # whisper weights pulled by faster-whisper (~500 MB)
  "$HOME_DIR/.cache/whisper"
)
echo "NOTE: the whisper model cache is shared with any other HuggingFace/whisper use on this machine;"
echo "      remove it only if you are not using those weights for anything else."
if [ "$KEEP_REPO" -eq 1 ]; then
  PATHS+=(
    "$ROOT/target"                           # rust build cache
    "$ROOT/ui/node_modules"
    "$ROOT/ui/.next"
    "$ROOT/eval/results/.cache"
  )
fi

size_of() {
  [ -e "$1" ] || { echo ""; return; }
  du -sh "$1" 2>/dev/null | cut -f1
}

total=0
printf '%-58s %8s\n' "PATH" "SIZE"
printf '%-58s %8s\n' "----------------------------------------------------------" "--------"
for p in "${PATHS[@]}"; do
  if [ -e "$p" ]; then
    s=$(size_of "$p")
    printf '%-58s %8s\n' "$p" "$s"
    kb=$(du -sk "$p" 2>/dev/null | cut -f1)
    total=$((total + kb))
  fi
done
printf '%-58s %8s\n' "TOTAL" "$(echo "$total" | awk '{printf "%.1fG", $1/1048576}')"
echo
echo "Processes:"
pgrep -a -f 'ollama|llama-server' 2>/dev/null | sed 's/^/  /' || echo "  none running"

if [ "$YES" -ne 1 ]; then
  echo
  echo "Dry run. Re-run with --yes to remove the paths above."
  echo "After removing, also: rotate the dev.to API key at https://dev.to/settings/extensions"
  exit 0
fi

echo
pkill -f 'ollama serve' 2>/dev/null && echo "stopped: ollama serve"
pkill -f 'llama-server' 2>/dev/null && echo "stopped: llama-server"
pkill -f 'target/debug/confidant-daemon' 2>/dev/null && echo "stopped: confidant daemon"
docker image rm confidant-demo 2>/dev/null && echo "removed: docker image confidant-demo"
sleep 1
for p in "${PATHS[@]}"; do
  [ -e "$p" ] || continue
  rm -rf "$p" && echo "removed: $p"
done
echo
echo "Remaining model dirs (should be empty):"
ls -d "$HOME_DIR/models" "$HOME_DIR/.ollama" 2>/dev/null || echo "  none"
free -h | head -2
