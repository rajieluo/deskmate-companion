"""Kyutai Pocket TTS sidecar for Deskmate Companion (see src-tauri/src/pocket.rs).

Loads the model once, then reads one JSON request per line on stdin and writes
one JSON message per line on stdout:

  -> {"id": 1, "cmd": "voice", "voice": "C:/.../voice.safetensors"}  preload a voice
  -> {"id": 2, "cmd": "speak", "voice": "...", "text": "Hi there."}
  -> {"cmd": "cancel", "upto": 2}         drop every request with id <= 2 (stops the current one)
  <- {"type": "ready", "rate": 24000, "load_ms": 900}
  <- {"id": 2, "type": "chunk", "pcm": "<base64 int16 little-endian mono>"}
  <- {"id": 2, "type": "done", "ms": 1450, "first_ms": 95, "audio_s": 4.3}
  <- {"id": 2, "type": "error", "message": "..."}

A voice is a preset name (alba, jane, ...), a .safetensors file made with
`pocket-tts export-voice`, or a .wav file, which is encoded once and cached in
--cache-dir as .safetensors. Requests run one at a time, in order. Logs go to
stderr. The process exits when stdin closes (the app quit or crashed).

Usage: python pocket_server.py [--quantize] [--cache-dir DIR]
"""
import argparse, base64, hashlib, json, os, queue, sys, threading, time
from pathlib import Path

# Keep stdout for the protocol only: anything else printed goes to the log.
PROTO = sys.stdout
sys.stdout = sys.stderr
_write = threading.Lock()


def send(msg):
    line = json.dumps(msg)
    with _write:
        PROTO.write(line + "\n")
        PROTO.flush()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quantize", action="store_true")
    ap.add_argument("--cache-dir", default=None)
    args = ap.parse_args()

    started = time.perf_counter()
    import torch
    from pocket_tts import TTSModel, export_model_state

    torch.set_num_threads(max(1, (os.cpu_count() or 4) // 2))
    model = TTSModel.load_model(quantize=args.quantize)
    send({"type": "ready", "rate": model.sample_rate, "load_ms": round((time.perf_counter() - started) * 1000)})

    cache = Path(args.cache_dir) if args.cache_dir else None
    states = {}

    def state_for(voice):
        if voice in states:
            return states[voice]
        t = time.perf_counter()
        src = voice
        p = Path(voice)
        if p.suffix.lower() in (".wav", ".mp3", ".flac", ".ogg") and cache:
            if not p.is_file():
                raise FileNotFoundError(f"voice file {voice} not found")
            st = p.stat()
            key = hashlib.sha1(f"{p.resolve()}|{st.st_size}|{st.st_mtime_ns}".encode()).hexdigest()[:16]
            cached = cache / f"{p.stem}-{key}.safetensors"
            if cached.is_file():
                src = str(cached)
            else:
                state = model.get_state_for_audio_prompt(str(p))
                cache.mkdir(parents=True, exist_ok=True)
                export_model_state(state, str(cached))
                print(f"encoded {p.name} and cached it as {cached}", flush=True)
                states[voice] = state
                return state
        elif p.suffix.lower() in (".wav", ".safetensors") and not p.is_file():
            raise FileNotFoundError(f"voice file {voice} not found")
        states[voice] = model.get_state_for_audio_prompt(src)
        print(f"voice {p.name if p.suffix else voice} ready in {time.perf_counter() - t:.2f} s", flush=True)
        return states[voice]

    requests = queue.Queue()
    cancelled = {"upto": 0}
    current = {"id": None, "stop": None}

    def reader():
        for line in sys.stdin:
            line = line.strip()
            if not line:
                continue
            try:
                msg = json.loads(line)
            except ValueError:
                print(f"bad request: {line[:200]}", flush=True)
                continue
            if msg.get("cmd") == "cancel":
                cancelled["upto"] = max(cancelled["upto"], int(msg.get("upto", 0)))
                stop, cid = current["stop"], current["id"]
                if stop is not None and cid is not None and cid <= cancelled["upto"]:
                    stop.set()
            else:
                requests.put(msg)
        requests.put(None)

    threading.Thread(target=reader, daemon=True).start()

    while True:
        msg = requests.get()
        if msg is None:
            break
        rid = msg.get("id")
        try:
            if rid is not None and rid <= cancelled["upto"]:
                send({"id": rid, "type": "done", "cancelled": True})
                continue
            state = state_for(msg["voice"])
            if msg.get("cmd") == "voice":
                send({"id": rid, "type": "done"})
                continue
            stop = threading.Event()
            current["id"], current["stop"] = rid, stop
            t = time.perf_counter()
            first, samples = None, 0
            for chunk in model.generate_audio_stream(state, msg["text"], stop=stop):
                if first is None:
                    first = time.perf_counter() - t
                pcm = (chunk.clamp(-1, 1) * 32767).to(torch.int16).numpy().tobytes()
                samples += chunk.shape[-1]
                send({"id": rid, "type": "chunk", "pcm": base64.b64encode(pcm).decode()})
            send({"id": rid, "type": "done", "ms": round((time.perf_counter() - t) * 1000),
                  "first_ms": round((first or 0) * 1000), "audio_s": round(samples / model.sample_rate, 2),
                  "cancelled": stop.is_set()})
        except Exception as e:  # report and keep serving
            send({"id": rid, "type": "error", "message": f"{type(e).__name__}: {e}"})
        finally:
            current["id"], current["stop"] = None, None


if __name__ == "__main__":
    main()
