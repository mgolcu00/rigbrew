// Dictation with the installed listening package (Sift): the microphone is open only between the
// two presses of the button, the recording is kept in memory as plain samples, turned into a WAV
// and sent to the local service, which writes it down. Nothing leaves the machine.

import { transcribe } from './api';
import { track } from './track';

type State = 'idle' | 'recording' | 'working';

/** Longest recording: ten minutes, the service's limit. */
const MAX_SECONDS = 600;

/** 16-bit PCM mono WAV from float samples. */
export function wav(samples: Float32Array, rate: number): Blob {
	const data = new DataView(new ArrayBuffer(44 + samples.length * 2));
	const text = (at: number, s: string) => [...s].forEach((c, i) => data.setUint8(at + i, c.charCodeAt(0)));
	text(0, 'RIFF');
	data.setUint32(4, 36 + samples.length * 2, true);
	text(8, 'WAVE');
	text(12, 'fmt ');
	data.setUint32(16, 16, true);
	data.setUint16(20, 1, true); // PCM
	data.setUint16(22, 1, true); // mono
	data.setUint32(24, rate, true);
	data.setUint32(28, rate * 2, true);
	data.setUint16(32, 2, true);
	data.setUint16(34, 16, true);
	text(36, 'data');
	data.setUint32(40, samples.length * 2, true);
	for (let i = 0; i < samples.length; i++) {
		const s = Math.max(-1, Math.min(1, samples[i]));
		data.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
	}
	return new Blob([data.buffer], { type: 'audio/wav' });
}

class Dictation {
	state = $state<State>('idle');
	startedAt = $state(0);
	error = $state<string | null>(null);

	#stream: MediaStream | null = null;
	#context: AudioContext | null = null;
	#node: ScriptProcessorNode | null = null;
	#chunks: Float32Array[] = [];
	#limit: ReturnType<typeof setTimeout> | null = null;

	/** Opens the microphone and starts keeping what it hears. */
	async start() {
		if (this.state !== 'idle') return;
		this.error = null;
		try {
			this.#stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
		} catch (e) {
			this.error =
				e instanceof DOMException && e.name === 'NotAllowedError'
					? 'Rigbrew may not use the microphone. Allow it in System Settings › Privacy & Security › Microphone, then try again.'
					: `The microphone could not be opened. ${e instanceof Error ? e.message : String(e)}`;
			return;
		}
		const context = (this.#context = new AudioContext());
		const source = context.createMediaStreamSource(this.#stream);
		// ScriptProcessorNode is old but works everywhere the app runs, with no worklet file to ship.
		const node = (this.#node = context.createScriptProcessor(4096, 1, 1));
		this.#chunks = [];
		node.onaudioprocess = (e) => this.#chunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
		source.connect(node);
		node.connect(context.destination);
		this.state = 'recording';
		this.startedAt = Date.now();
		this.#limit = setTimeout(() => this.stop(), MAX_SECONDS * 1000);
	}

	/** Closes the microphone and returns what was said, or '' when nothing was. */
	async stop(language?: string): Promise<string> {
		if (this.state !== 'recording') return '';
		if (this.#limit) clearTimeout(this.#limit);
		const rate = this.#context?.sampleRate ?? 48000;
		this.#node?.disconnect();
		this.#stream?.getTracks().forEach((t) => t.stop());
		await this.#context?.close().catch(() => {});
		this.#node = null;
		this.#stream = null;
		this.#context = null;
		const length = this.#chunks.reduce((n, c) => n + c.length, 0);
		const samples = new Float32Array(length);
		let at = 0;
		for (const c of this.#chunks) {
			samples.set(c, at);
			at += c.length;
		}
		this.#chunks = [];
		const seconds = length / rate;
		if (seconds < 0.4) {
			this.state = 'idle';
			return '';
		}
		this.state = 'working';
		try {
			const heard = await transcribe(wav(samples, rate), language);
			track('dictated', { audio_seconds: Math.round(seconds), seconds: Math.round(heard.seconds * 10) / 10, language: heard.language ?? '' });
			return heard.text;
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			return '';
		} finally {
			this.state = 'idle';
		}
	}

	/** Drops a recording without writing it down. */
	cancel() {
		if (this.#limit) clearTimeout(this.#limit);
		this.#node?.disconnect();
		this.#stream?.getTracks().forEach((t) => t.stop());
		this.#context?.close().catch(() => {});
		this.#node = null;
		this.#stream = null;
		this.#context = null;
		this.#chunks = [];
		this.state = 'idle';
	}
}

export const dictation = new Dictation();
