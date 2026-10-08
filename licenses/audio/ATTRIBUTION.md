# Audio component attribution

@sapphi-red/web-noise-suppressor 0.4.1, Copyright (c) 2022 翠 / green (MIT).
WebTS adds initialization ready/error messages to its pinned RNNoise and GTCRN worklets.
The optional voice-only mode uses the existing RNNoise speech probability to apply
a local 10ms lookback, speech hangover and smooth gain envelope after denoising.
The underlying WASM models and their noise reduction algorithms are unchanged.

The distributed WASM binaries use @shiguredo/rnnoise-wasm 2022.2.0 (Apache-2.0):
Copyright 2021-2021, Takeru Ohta (Original Author).
Copyright 2021-2021, Shiguredo Inc.
Underlying shiguredo/rnnoise 2022.1.0 uses the included BSD-3-Clause COPYING with its original authors.

SIMD detection uses wasm-feature-detect 1.9.0 by GoogleChromeLabs (Apache-2.0).
Full original license texts are bundled alongside this file.

Keyboard enhancement uses the same fixed package's GTCRN worklet and WASM:
@sapphi-red/gtcrn-wasm 0.0.3 (MIT), Copyright (c) 2026 sapphi-red.
Model: GTCRN, Copyright (c) 2024 Rong Xiaobin (MIT), upstream commit
3862c44808dca492ea5a8a145d2dc2a1028d08c8.
Its WASM includes PFFFT/FFTPACK under the bundled UCAR license, commit
a4b03590cc2a4bea56f9721996e3057835799179. PFFFT source attribution also includes
Copyright (c) 2020 Hayati Ayguen; original FFTPACKv4 by Dr Paul Swarztrauber.
Model code was generated with onnx2c bfd0753ac0ad32ce6ccd324add61ff5505a68c02;
its original permissive license and author list are included (not relabeled MIT).

GTCRN internally processes 16 kHz speech and resamples its 48 kHz interface.
It does not perform acoustic echo cancellation. Neither model uses a WebTS
amplitude threshold or keyboard-event mute rule to stop transmission.
