import * as cryptoClient from '../../src/friend-crypto';
import {createReceiver,createPlaybackLimiter} from '../../src/receive-audio';
import {Voice} from '../../src/voice';
import {defaultAudioSettings} from '../../src/audio-settings';
import {createNoise} from '../../src/noise';
import {createStreamNoise,supportsStreamNoise} from '../../src/stream-noise';
// Served by the loopback-only test Vite instance; not an entry in the production build.
Object.assign(window,{cryptoClient,audioFixture:{createReceiver,createPlaybackLimiter,createNoise,createStreamNoise,supportsStreamNoise,Voice,defaultAudioSettings}});
