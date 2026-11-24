# Sets up Kyutai Pocket TTS (cloned voices) for Deskmate Companion.
#
# The app runs this from the tray (Voice > Set up Pocket TTS...). It needs no
# Python on the PC and no admin rights. Everything goes into the app's data folder:
#
#   pocket\uv\      uv, a small tool that downloads Python and packages
#   pocket\python\  a private copy of Python 3.12 (not added to PATH)
#   pocket\venv\    a virtual environment with pocket-tts and PyTorch (CPU), ~800 MB
#
# Then, optionally, it logs in to Hugging Face so Pocket TTS can clone voices from
# .wav recordings (that model is gated: you accept its terms with your own account).
#
# Running it again is safe: finished steps are skipped. To remove everything,
# delete the pocket folder.

param(
    [string]$DataDir = (Join-Path $env:APPDATA 'com.rajieluo.desktopassistant'),
    # Skip the Hugging Face questions (for scripted installs).
    [switch]$NoLogin
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
try { $Host.UI.RawUI.WindowTitle = 'Deskmate Companion: Pocket TTS setup' } catch {}

$pocket = Join-Path $DataDir 'pocket'
$venv = Join-Path $pocket 'venv'
$python = Join-Path $venv 'Scripts\python.exe'
$uvDir = Join-Path $pocket 'uv'
# Keep uv's Python and download cache inside the pocket folder too.
$env:UV_PYTHON_INSTALL_DIR = Join-Path $pocket 'python'
$env:UV_CACHE_DIR = Join-Path $pocket 'uv-cache'
$env:UV_NO_MODIFY_PATH = '1'
# Always use uv's own Python, so the venv doesn't break if a system Python is uninstalled.
$env:UV_PYTHON_PREFERENCE = 'only-managed'

function Step($text) { Write-Host ''; Write-Host "== $text" -ForegroundColor Cyan }
function Check($what) { if ($LASTEXITCODE -ne 0) { throw "$what failed (exit code $LASTEXITCODE)" } }

# Runs a native command with its output hidden. Windows PowerShell turns
# redirected stderr into errors under 'Stop', so relax that just for the call.
function Quiet([scriptblock]$command) {
    $saved = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try { & $command *> $null } finally { $ErrorActionPreference = $saved }
    return $LASTEXITCODE
}

function Finish($code) {
    Write-Host ''
    Read-Host 'Press Enter to close this window' | Out-Null
    exit $code
}

try {
    Write-Host 'Deskmate Companion: setting up Pocket TTS (cloned voices)' -ForegroundColor Green
    Write-Host "Folder: $pocket"
    Write-Host 'This downloads about 1 GB the first time and takes a few minutes.'
    New-Item -ItemType Directory -Force -Path $pocket | Out-Null

    Step '1/4  uv (downloads Python and packages)'
    $uv = Join-Path $uvDir 'uv.exe'
    if (Test-Path $uv) {
        Write-Host "Already there: $uv"
    } else {
        $zip = Join-Path $pocket 'uv.zip'
        # Pinned uv release and its SHA-256, so a swapped download is refused.
        $url = 'https://github.com/astral-sh/uv/releases/download/0.13.9/uv-x86_64-pc-windows-msvc.zip'
        $sha256 = 'edd61f7f5728a56cc62455386661860bdfc7bab65872f39b0c25bdcc1d8e1beb'
        Write-Host "Downloading $url"
        Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
        $got = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
        if ($got -ne $sha256) {
            Remove-Item -Force $zip
            throw "The uv download doesn't match its checksum (got $got), so it wasn't installed"
        }
        $unpacked = Join-Path $pocket 'uv-unpack'
        if (Test-Path $unpacked) { Remove-Item -Recurse -Force $unpacked }
        Expand-Archive -Path $zip -DestinationPath $unpacked -Force
        $found = Get-ChildItem -Path $unpacked -Recurse -Filter 'uv.exe' | Select-Object -First 1
        if (-not $found) { throw 'uv.exe was not in the download' }
        New-Item -ItemType Directory -Force -Path $uvDir | Out-Null
        Get-ChildItem -Path $found.DirectoryName -File | Copy-Item -Destination $uvDir -Force
        Remove-Item -Recurse -Force $unpacked, $zip
        Write-Host "Installed $uv"
    }

    Step '2/4  Python 3.12 and a virtual environment'
    if (Test-Path $python) {
        Write-Host "Already there: $python"
    } else {
        & $uv venv --python 3.12 $venv
        Check 'Creating the virtual environment'
    }

    Step '3/4  Pocket TTS and PyTorch (the big download)'
    & $uv pip install --python $python pocket-tts
    Check 'Installing pocket-tts'
    & $python -I -c "import pocket_tts, torch; print('pocket-tts is ready (PyTorch ' + torch.__version__ + ')')"
    Check 'Loading pocket-tts'
    # The app looks here first (see pocket.rs), so it finds this Python even if
    # settings.json points somewhere else that no longer exists.
    [IO.File]::WriteAllText((Join-Path $pocket 'python.txt'), $python)
    # The downloaded packages are already in the venv; their cache is ~700 MB more.
    Quiet { & $uv cache clean --quiet } | Out-Null

    Step '4/4  Hugging Face login (for cloning voices from recordings)'
    $hf = Join-Path $venv 'Scripts\hf.exe'
    $loggedIn = $false
    if (Test-Path $hf) {
        $loggedIn = ((Quiet { & $hf auth whoami }) -eq 0)
    }
    if ($loggedIn) {
        Write-Host 'Already logged in to Hugging Face.'
    } elseif ($NoLogin) {
        Write-Host 'Skipped (-NoLogin).'
    } else {
        Write-Host 'Pocket TTS works now with its built-in voices (alba, jane, ...).'
        Write-Host 'To clone a voice from a .wav recording it needs a gated model that'
        Write-Host 'you unlock with your own free Hugging Face account:'
        Write-Host '  1. Sign in at huggingface.co and accept the terms on the kyutai/pocket-tts page.'
        Write-Host '  2. Create a token with "Read" access under Settings > Access Tokens.'
        Write-Host '  3. Paste it below.'
        $answer = Read-Host 'Open those pages and log in now? (y/n)'
        if ($answer -match '^(y|yes)$') {
            Start-Process 'https://huggingface.co/kyutai/pocket-tts'
            Start-Process 'https://huggingface.co/settings/tokens'
            if (-not (Test-Path $hf)) {
                & $uv pip install --python $python huggingface_hub
                Check 'Installing huggingface_hub'
            }
            & $hf auth login
            if ($LASTEXITCODE -ne 0) {
                Write-Host 'Login did not finish. Run this setup again later to retry.' -ForegroundColor Yellow
            }
        } else {
            Write-Host 'Skipped. Run this setup again any time to log in.'
        }
    }

    Write-Host ''
    Write-Host 'Done. In the tray, pick Voice > Engine: Pocket TTS (cloned voice).' -ForegroundColor Green
    Write-Host 'A model uses Pocket TTS when its folder has voice.safetensors, voice.wav,'
    Write-Host 'or a pocket_voice: line in persona.md. Other models keep the Kokoro voice.'
    Finish 0
} catch {
    Write-Host ''
    Write-Host "Setup failed: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host 'Kokoro keeps working in the meantime. Fix the problem above and run the setup again.'
    Finish 1
}
