#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
生成一段“调性非常明确”的 House 风格 WAV（用于本项目的调性检测调参/回归）。

特点：
- 固定调性：默认 C#/Db Major（Camelot 3B）
- 包含 4/4 Kick + Clap + Hat + Bass + Chords，便于观察 HPSS/N/A(无调性) 门限
- 不依赖第三方库（仅标准库）

用法示例：
  python "scripts/generate_tonal_house_sample.py"
  python "scripts/generate_tonal_house_sample.py" --seconds 90 --bpm 124 --out "tmp/house_csharp_major_124bpm.wav"
"""

from __future__ import annotations

import argparse
import math
import os
import random
import struct
import wave


def midi_to_hz(midi: float) -> float:
    return 440.0 * (2.0 ** ((midi - 69.0) / 12.0))


def clamp(x: float, lo: float, hi: float) -> float:
    return lo if x < lo else hi if x > hi else x


def exp_decay(t: float, tau: float) -> float:
    if tau <= 0.0:
        return 0.0
    return math.exp(-t / tau)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="tmp/house_csharp_major_124bpm.wav", help="输出 WAV 路径")
    ap.add_argument("--seconds", type=float, default=60.0, help="时长（秒）")
    ap.add_argument("--bpm", type=float, default=124.0, help="BPM")
    ap.add_argument("--sr", type=int, default=44100, help="采样率")
    args = ap.parse_args()

    out_path = args.out
    seconds = float(args.seconds)
    bpm = float(args.bpm)
    sr = int(args.sr)
    if seconds <= 0 or bpm <= 0 or sr <= 0:
        raise SystemExit("invalid args")

    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    random.seed(42)

    beat_sec = 60.0 / bpm
    bar_sec = beat_sec * 4.0
    total = int(sr * seconds)

    # C#/Db Major：I–V–vi–IV（每小节换和弦，循环）
    # 这里用等音名（B#=C, E#=F）避免符号复杂度，但音高正确。
    chords_midi = [
        # C# major: C#4 F4 G#4
        (61, 65, 68),
        # G# major: G#3 C4 D#4
        (56, 60, 63),
        # A# minor: A#3 C#4 F4
        (58, 61, 65),
        # F# major: F#3 A#3 C#4
        (54, 58, 61),
    ]
    bass_root_midi = [37, 44, 46, 42]  # C#1, G#1, A#1, F#1（低八度 root）

    # 电平（避免削顶）：chords/bass 会被 sidechain 衰减
    amp_ch = 0.18
    amp_bass = 0.22
    amp_kick = 0.55
    amp_clap = 0.16
    amp_hat = 0.10

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
                beat_pos = t / beat_sec
                bar_pos = t / bar_sec

                # sidechain：每拍 kick 后快速衰减、缓慢恢复（模拟 house 泵感）
                t_in_beat = t % beat_sec
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
                t_in_bar = t % bar_sec
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

                # master limiter
                x = clamp(x, -1.0, 1.0)
                s16 = int(round(x * 32767.0))
                buf += struct.pack("<h", s16)
            wf.writeframes(buf)

    print(f"[sample] wrote: {out_path}")
    print("[sample] expected key: C#/Db Major (Camelot 3B)")
    print(f"[sample] bpm={bpm:g} sr={sr} seconds={seconds:g}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

