#!/usr/bin/env bash
# Sets up Kyutai Pocket TTS (cloned voices) for Deskmate Companion on Linux.
# The Linux twin of setup-pocket.ps1.
#
# The app runs this from the tray (Voice > Set up Pocket TTS...) in a terminal
# window. It needs no system Python and no root. Everything goes into the app's
# data folder (~/.local/share/com.rajieluo.desktopassistant by default):
#
#   pocket/uv/      uv, a small tool that downloads Python and packages
#   pocket/python/  a private copy of Python 3.12
#   pocket/venv/    a virtual environment with pocket-tts and PyTorch (CPU), ~800 MB
#
# Then, optionally, it logs in to Hugging Face so Pocket TTS can clone voices from
# .wav recordings (that model is gated: you accept its terms with your own account).
#
# Running it again is safe: finished steps are skipped. To remove everything,
# delete the pocket folder.
#
# Usage: setup-pocket.sh [--data-dir DIR] [--no-login]

set -u

data_dir="${XDG_DATA_HOME:-$HOME/.local/share}/com.rajieluo.desktopassistant"
no_login=0
while [ $# -gt 0 ]; do
  case $1 in
    --data-dir) data_dir=$2; shift 2 ;;
    --no-login) no_login=1; shift ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

pocket=$data_dir/pocket
venv=$pocket/venv
python=$venv/bin/python
uv_dir=$pocket/uv
uv=$uv_dir/uv
# Keep uv's Python and download cache inside the pocket folder too.
export UV_PYTHON_INSTALL_DIR=$pocket/python
export UV_CACHE_DIR=$pocket/uv-cache
export UV_NO_MODIFY_PATH=1
# Always use uv's own Python, so the venv doesn't break if a system Python changes.
export UV_PYTHON_PREFERENCE=only-managed
# PyTorch's CPU build: the default Linux wheel pulls in ~3 GB of CUDA libraries.
export UV_TORCH_BACKEND=cpu

printf '\033]0;Deskmate Companion: Pocket TTS setup\007'
cyan=$'\033[36m' green=$'\033[32m' yellow=$'\033[33m' red=$'\033[31m' plain=$'\033[0m'
step() { printf '\n%s== %s%s\n' "$cyan" "$1" "$plain"; }

finish() {
  echo
  read -r -p 'Press Enter to close this window' _ || true
  exit "$1"
}

fail() {
  printf '\n%sSetup failed: %s%s\n' "$red" "$1" "$plain"
  echo 'Kokoro keeps working in the meantime. Fix the problem above and run the setup again.'
  finish 1
}

open_url() { xdg-open "$1" >/dev/null 2>&1 || echo "Open $1 in your browser."; }

printf '%sDeskmate Companion: setting up Pocket TTS (cloned voices)%s\n' "$green" "$plain"
echo "Folder: $pocket"
echo 'This downloads about 1 GB the first time and takes a few minutes.'
mkdir -p "$pocket" || fail "Couldn't create $pocket"

step '1/4  uv (downloads Python and packages)'
if [ -x "$uv" ]; then
  echo "Already there: $uv"
else
  # Pinned uv release and its SHA-256, so a swapped download is refused.
  case $(uname -m) in
    x86_64|amd64) target=x86_64-unknown-linux-gnu sha256=a7fbaff2b9a271d85c1bdb540941090fd1ce011797b4abeb48e4dcd39c2471d5 ;;
    aarch64|arm64) target=aarch64-unknown-linux-gnu sha256=0797542018e541a9288855df555c8077eed06f7a47d206362030480e8ecc9f3d ;;
    *) fail "No uv download for this CPU ($(uname -m))" ;;
  esac
  url=https://github.com/astral-sh/uv/releases/download/0.13.9/uv-$target.tar.gz
  archive=$pocket/uv.tar.gz
  echo "Downloading $url"
  if command -v curl >/dev/null 2>&1; then
    curl -fL --progress-bar -o "$archive" "$url" || fail 'Downloading uv'
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O "$archive" "$url" || fail 'Downloading uv'
  else
    fail 'Neither curl nor wget is installed (install one with your package manager)'
  fi
  got=$(sha256sum "$archive" 2>/dev/null || shasum -a 256 "$archive")
  if [ "${got%% *}" != "$sha256" ]; then
    rm -f "$archive"
    fail "The uv download doesn't match its checksum (got ${got%% *}), so it wasn't installed"
  fi
  mkdir -p "$uv_dir" || fail "Couldn't create $uv_dir"
  tar -xzf "$archive" -C "$uv_dir" --strip-components=1 || fail 'Unpacking uv'
  rm -f "$archive"
  [ -x "$uv" ] || fail "uv was not in the download"
  echo "Installed $uv"
fi

step '2/4  Python 3.12 and a virtual environment'
if [ -x "$python" ]; then
  echo "Already there: $python"
else
  "$uv" venv --python 3.12 "$venv" || fail 'Creating the virtual environment'
fi

step '3/4  Pocket TTS and PyTorch (the big download)'
"$uv" pip install --python "$python" pocket-tts || fail 'Installing pocket-tts'
"$python" -I -c "import pocket_tts, torch; print('pocket-tts is ready (PyTorch ' + torch.__version__ + ')')" \
  || fail 'Loading pocket-tts'
# The app looks here first (see pocket.rs), so it finds this Python even if
# settings.json points somewhere else that no longer exists.
printf '%s' "$python" >"$pocket/python.txt"
# The downloaded packages are already in the venv; their cache is ~700 MB more.
"$uv" cache clean --quiet >/dev/null 2>&1 || true

step '4/4  Hugging Face login (for cloning voices from recordings)'
hf=$venv/bin/hf
if [ -x "$hf" ] && "$hf" auth whoami >/dev/null 2>&1; then
  echo 'Already logged in to Hugging Face.'
elif [ "$no_login" = 1 ]; then
  echo 'Skipped (--no-login).'
else
  echo 'Pocket TTS works now with its built-in voices (alba, jane, ...).'
  echo 'To clone a voice from a .wav recording it needs a gated model that'
  echo 'you unlock with your own free Hugging Face account:'
  echo '  1. Sign in at huggingface.co and accept the terms on the kyutai/pocket-tts page.'
  echo '  2. Create a token with "Read" access under Settings > Access Tokens.'
  echo '  3. Paste it below.'
  read -r -p 'Open those pages and log in now? (y/n) ' answer || answer=n
  case $answer in
    y|Y|yes|Yes)
      open_url https://huggingface.co/kyutai/pocket-tts
      open_url https://huggingface.co/settings/tokens
      if [ ! -x "$hf" ]; then
        "$uv" pip install --python "$python" huggingface_hub || fail 'Installing huggingface_hub'
      fi
      "$hf" auth login || printf '%sLogin did not finish. Run this setup again later to retry.%s\n' "$yellow" "$plain"
      ;;
    *) echo 'Skipped. Run this setup again any time to log in.' ;;
  esac
fi

echo
printf '%sDone. In the tray, pick Voice > Engine: Pocket TTS (cloned voice).%s\n' "$green" "$plain"
echo 'A model uses Pocket TTS when its folder has voice.safetensors, voice.wav,'
echo 'or a pocket_voice: line in persona.md. Other models keep the Kokoro voice.'
finish 0
