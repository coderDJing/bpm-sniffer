#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成一段“调性非常明确”的 House 风格 WAV（用于本项目的调性检测调参/回归）。

特点：
- 默认生成 24 个调性样本（12 大调 + 12 小调）
- 可用 --single 指定单一调性（默认 C#/Db Major，Camelot 3B）
- 默认时长：300 秒
- I–IV–V–I 循环 + 主音持续音 + 音阶旋律，明确主音中心（真值样本）
- 文件名包含 Camelot 标识（如 house_3B_Csharp_major_124bpm.wav）
- 包含 4/4 Kick + Clap + Hat + Bass + Chords，便于观察 HPSS/N/A(无调性) 门限
- 不依赖第三方库（仅标准库）

用法示例：
  python "scripts/generate_tonal_house_sample.py"
  python "scripts/generate_tonal_house_sample.py" --single --key "F#" --mode major
  python "scripts/generate_tonal_house_sample.py" --keys "C,C#,Dm,F#m"
"""

from __future__ import annotations

import argparse
import math
import os
import random
import struct
import wave

PITCH_NAMES_SHARP = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
PITCH_NAMES_FILE = ["C", "Csharp", "D", "Dsharp", "E", "F", "Fsharp", "G", "Gsharp", "A", "Asharp", "B"]
FLAT_TO_SHARP = {"Db": "C#", "Eb": "D#", "Gb": "F#", "Ab": "G#", "Bb": "A#"}

MAJOR_CAMELOT = ["8B", "3B", "10B", "5B", "12B", "7B", "2B", "9B", "4B", "11B", "6B", "1B"]
MINOR_CAMELOT = ["5A", "12A", "7A", "2A", "9A", "4A", "11A", "6A", "1A", "8A", "3A", "10A"]


def midi_to_hz(midi: float) -> float:
    return 440.0 * (2.0 ** ((midi - 69.0) / 12.0))


def clamp(x: float, lo: float, hi: float) -> float:
    return lo if x < lo else hi if x > hi else x


def exp_decay(t: float, tau: float) -> float:
    if tau <= 0.0:
        return 0.0
    return math.exp(-t / tau)


def normalize_mode(mode: str) -> str:
    m = mode.strip().lower()
    if m in ("major", "maj"):
        return "major"
    if m in ("minor", "min", "m"):
        return "minor"
    raise SystemExit(f"invalid mode: {mode}")


def parse_key_token(token: str, default_mode: str) -> tuple[int, str]:
    s = token.strip()
    if not s:
        raise SystemExit("empty key token")
    s_low = s.lower()
    mode = default_mode
    if s_low.endswith("major"):
        mode = "major"
        s = s[:-5]
    elif s_low.endswith("maj"):
        mode = "major"
        s = s[:-3]
    elif s_low.endswith("minor"):
        mode = "minor"
        s = s[:-5]
    elif s_low.endswith("min"):
        mode = "minor"
        s = s[:-3]
    elif s_low.endswith("m") and len(s) > 1:
        mode = "minor"
        s = s[:-1]

    s = s.strip()
    if not s:
        raise SystemExit("invalid key token")

    note = s[0].upper()
    accidental = ""
    if len(s) >= 2 and s[1] in ("#", "b", "B"):
        accidental = s[1]
        rest = s[2:].strip()
    else:
        rest = s[1:].strip()
    if rest:
        raise SystemExit(f"invalid key token: {token}")

    name = note + accidental
    if name.endswith("b") or name.endswith("B"):
        name = name[0] + "b"
        name = FLAT_TO_SHARP.get(name, name)

    if name not in PITCH_NAMES_SHARP:
        raise SystemExit(f"invalid key token: {token}")

    pc = PITCH_NAMES_SHARP.index(name)
    return pc, normalize_mode(mode)


def parse_key_list(raw: str, default_mode: str) -> list[tuple[int, str]]:
    if not raw.strip():
        return []
    out = []
    for token in raw.split(","):
        token = token.strip()
        if token:
            out.append(parse_key_token(token, default_mode))
    if not out:
        raise SystemExit("empty key list")
    return out


def key_label(pc: int, mode: str) -> str:
    name = PITCH_NAMES_SHARP[pc]
    return f"{name}m" if mode == "minor" else name


def camelot_label(pc: int, mode: str) -> str:
    return MINOR_CAMELOT[pc] if mode == "minor" else MAJOR_CAMELOT[pc]


def key_filename(pc: int, mode: str, bpm: float) -> str:
    name = PITCH_NAMES_FILE[pc]
    suffix = "minor" if mode == "minor" else "major"
    camelot = camelot_label(pc, mode)
    return f"house_{camelot}_{name}_{suffix}_{bpm:g}bpm.wav"


def build_triad(root_midi: int, quality: str) -> tuple[int, int, int]:
    third = 3 if quality == "min" else 4
    return (root_midi + 12, root_midi + 12 + third, root_midi + 12 + 7)


def build_progression(tonic_midi: int, mode: str) -> tuple[list[tuple[int, int, int]], list[int]]:
    if mode == "major":
        degrees = [(0, "maj"), (5, "maj"), (7, "maj"), (0, "maj")]
    else:
        degrees = [(0, "min"), (5, "min"), (7, "maj"), (0, "min")]
    chords = []
    bass_roots = []
    for interval, quality in degrees:
        root = tonic_midi + interval
        chords.append(build_triad(root, quality))
        bass_roots.append(root)
    return chords, bass_roots


def build_melody(tonic_midi: int, mode: str) -> list[int]:
    if mode == "major":
        steps = [0, 2, 4, 5, 7, 9, 11, 12]
    else:
        steps = [0, 2, 3, 5, 7, 8, 11, 12]  # harmonic minor
    base = tonic_midi + 24
    return [base + s for s in steps]


def seed_for_key(pc: int, mode: str) -> int:
    return 42 + pc + (0 if mode == "major" else 100)


def render_sample(out_path: str, seconds: float, bpm: float, sr: int, pc: int, mode: str) -> None:
    if seconds <= 0 or bpm <= 0 or sr <= 0:
        raise SystemExit("invalid args")

    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    random.seed(seed_for_key(pc, mode))

    beat_sec = 60.0 / bpm
    bar_sec = beat_sec * 4.0
    total = int(sr * seconds)

    tonic_midi = 48 + pc  # C3=48，保证低频仍在可分析范围内
    chords_midi, bass_root_midi = build_progression(tonic_midi, mode)
    melody_midi = build_melody(tonic_midi, mode)

    # 电平（避免削顶）：chords/bass 会被 sidechain 衰减
    amp_ch = 0.18
    amp_bass = 0.22
    amp_kick = 0.55
    amp_clap = 0.16
    amp_hat = 0.10
    amp_tonic = 0.06
    amp_melody = 0.12

    with wave.open(out_path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)  # 16-bit PCM
        wf.setframerate(sr)

        chunk = 2048
        buf = bytearray()
        two_pi = 2.0 * math.pi

        for n0 in range(0, total, chunk):
            n1 = min(total, n0 + chunk)
            buf.clear()
            for n in range(n0, n1):
                t = n / sr
                bar_pos = t / bar_sec

                # sidechain：每拍 kick 后快速衰减、缓慢恢复（模拟 house 泵感）
                t_in_beat = t % beat_sec
                t_in_bar = t % bar_sec
                sc = 1.0 - 0.65 * exp_decay(t_in_beat, 0.12)
                sc = clamp(sc, 0.25, 1.0)

                # 当前和弦/贝斯根音（每小节换一次）
                chord_idx = int(bar_pos) % len(chords_midi)
                chord = chords_midi[chord_idx]
                bass_midi = bass_root_midi[chord_idx]

                x = 0.0

                # Kick：每拍一次，短扫频 + 指数包络
                if t_in_beat < 0.16:
                    kt = t_in_beat
                    f0 = 95.0
                    f1 = 52.0
                    fk = f1 + (f0 - f1) * exp_decay(kt, 0.035)
                    x += amp_kick * exp_decay(kt, 0.07) * math.sin(two_pi * fk * kt)

                # Clap：第 2/4 拍（0-based: 1, 3）
                clap_t = None
                if 0.0 <= t_in_bar - beat_sec * 1.0 < 0.09:
                    clap_t = t_in_bar - beat_sec * 1.0
                elif 0.0 <= t_in_bar - beat_sec * 3.0 < 0.09:
                    clap_t = t_in_bar - beat_sec * 3.0
                if clap_t is not None:
                    env = exp_decay(clap_t, 0.035)
                    noise = (random.random() * 2.0 - 1.0)
                    x += amp_clap * env * noise

                # Hat：每 1/2 拍一次（8 分音符），高频噪声短包络
                hat_grid = beat_sec * 0.5
                t_in_hat = t % hat_grid
                if t_in_hat < 0.03:
                    env = exp_decay(t_in_hat, 0.012)
                    noise = (random.random() * 2.0 - 1.0)
                    x += amp_hat * env * noise

                # Bass：root + 2nd harmonic（简化的“锯齿感”）
                fb = midi_to_hz(bass_midi)
                bt = t % beat_sec
                b_env = 0.65 * exp_decay(bt, 0.22) + 0.35
                x += sc * amp_bass * b_env * (
                    0.75 * math.sin(two_pi * fb * t) + 0.25 * math.sin(two_pi * (2.0 * fb) * t)
                )

                # Chords：三音叠加 + 轻微 detune，整小节保持
                for i, m in enumerate(chord):
                    fc = midi_to_hz(float(m) + (i - 1) * 0.01)
                    x += sc * (amp_ch / 3.0) * math.sin(two_pi * fc * t)

                # 主音持续音：常驻 tonic，减少相对大小调摇摆
                ft = midi_to_hz(float(tonic_midi))
                x += amp_tonic * math.sin(two_pi * ft * t)

                # 旋律：8 分音符音阶，每小节回到主音
                step_len = beat_sec * 0.5
                step_idx = int(t_in_bar / step_len) % len(melody_midi)
                mt = t_in_bar - step_len * step_idx
                if mt < step_len:
                    env = exp_decay(mt, 0.10)
                    fm = midi_to_hz(float(melody_midi[step_idx]))
                    x += amp_melody * env * math.sin(two_pi * fm * t)

                # master limiter
                x = clamp(x, -1.0, 1.0)
                s16 = int(round(x * 32767.0))
                buf += struct.pack("<h", s16)
            wf.writeframes(buf)

    key = key_label(pc, mode)
    camelot = camelot_label(pc, mode)
    print(f"[sample] wrote: {out_path}")
    print(f"[sample] expected key: {key} (Camelot {camelot})")
    print(f"[sample] bpm={bpm:g} sr={sr} seconds={seconds:g}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="", help="输出 WAV 路径（单样本，留空则自动命名）")
    ap.add_argument("--out-dir", default="", help="输出目录（多样本/单样本自动命名时使用）")
    ap.add_argument("--seconds", type=float, default=300.0, help="时长（秒）")
    ap.add_argument("--bpm", type=float, default=124.0, help="BPM")
    ap.add_argument("--sr", type=int, default=44100, help="采样率")
    ap.add_argument("--key", default="C#", help="主音（C/C#/Db/...）")
    ap.add_argument("--mode", default="major", help="major/minor")
    ap.add_argument("--keys", default="", help="逗号分隔的调性列表（如 \"C,C#,Dm,F#m\"）")
    ap.add_argument("--all-keys", action="store_true", help="生成 24 个调性样本（12 大调 + 12 小调）")
    ap.add_argument("--single", action="store_true", help="生成单一调性样本")
    args = ap.parse_args()

    seconds = float(args.seconds)
    bpm = float(args.bpm)
    sr = int(args.sr)
    mode = normalize_mode(args.mode)

    if args.single and (args.all_keys or args.keys.strip()):
        raise SystemExit("use --single or --all-keys/--keys, not both")
    if args.all_keys and args.keys.strip():
        raise SystemExit("use --all-keys or --keys, not both")

    if args.single:
        pc, kmode = parse_key_token(args.key, mode)
        out_path = args.out
        if not out_path:
            out_dir = args.out_dir or f"tmp/keys_{bpm:g}bpm"
            os.makedirs(out_dir, exist_ok=True)
            out_path = os.path.join(out_dir, key_filename(pc, kmode, bpm))
        render_sample(out_path, seconds, bpm, sr, pc, kmode)
        return 0

    if args.out:
        raise SystemExit("--out only works with --single")

    if args.keys.strip():
        keys = parse_key_list(args.keys, mode)
    else:
        keys = [(pc, "major") for pc in range(12)] + [(pc, "minor") for pc in range(12)]

    out_dir = args.out_dir or f"tmp/keys_{bpm:g}bpm"
    os.makedirs(out_dir, exist_ok=True)
    for pc, kmode in keys:
        out_path = os.path.join(out_dir, key_filename(pc, kmode, bpm))
        render_sample(out_path, seconds, bpm, sr, pc, kmode)
    print(f"[batch] wrote {len(keys)} files into: {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

