# Voice processing

On browsers exposing audio MediaStreamTrackProcessor/Generator and Worker,
the enhanced mode uses browser noise suppression plus a local RNNoise 2025
worker. It assembles 480-sample / 10 ms frames at 48 kHz and sends the generated
track directly, avoiding a capture AudioContext destination queue. The initial
one second primes the model before the connection accepts transmission; that
history is discarded, never sent later. This is not a 10 ms TeamSpeak packet
change: the gateway and existing 20 ms interoperability remain unchanged.

Unsupported browsers retain the compatible RNNoise/GTCRN path. Failed modern
initialization first disables browser suppression before loading the compatible
chain; if that fails, sending stays blocked. Runtime processor failure blocks
sending before reacquiring compatible capture. Receiving and channel state are
retained. AEC, AGC, mute, gain and encrypted transport retain their controls.
The strength slider controls AI enhancement; native suppression remains active
in the realtime mode. Turning suppression off with unity gain sends the original
capture track; other gain values retain the gain graph.

The model is loaded from same-origin static resources, not a cloud inference
service. Older immutable voice6/7/8 resources remain available for cached pages.
Publish all three new audio assets with this revision; packaging rejects their
absence. Keep both wrapper Apache-2.0 and embedded RNNoise BSD notices.

## Verification and limits

Automated tests cover processing replacement, mute/AFK, gain, device recovery,
startup clock rebasing, runtime discontinuity rejection, teardown and fallback.
Real Edge desktop/mobile-emulation tests exercise actual workers, RTC audio,
gain ratios, bidirectional receiving, mute and compatible fallback. Emulation is
not physical mobile acceptance. Short controlled Beijing captures show reduced
capture processing delay; no universal mouth-to-ear latency is asserted.

Synthetic-noise comparisons found stronger steady keyboard/hum suppression but
weaker steady fan suppression than the old GTCRN enhancement (approximately
55 dB versus 66 dB attenuation in that fixture). Noise introduced after silence
still requires adaptation. These fixtures do not establish subjective speech
quality or superiority in every environment. The compatible enhancement remains
available as an automatic fallback, not an equivalent benchmark claim.

The gateway binary, node protocol and standard voice encryption are unchanged.
The release remains a candidate pending physical-device and end-to-end latency
acceptance. Retain the previous frontend image for rollback.
