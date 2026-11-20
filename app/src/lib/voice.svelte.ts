// Reading answers aloud with the installed voice package (Whistle). One reading plays at a time;
// a reading is kept for the last few answers, so playing one again does not generate it again.

import { speak } from './api';
import { track } from './track';

type State = 'idle' | 'preparing' | 'playing';

const KEEP = 8;

/**
 * Whether a text looks like English, the only language the voice reads well today. Letters
 * outside basic Latin (ç, ğ, ı, ö, ş, ü, é, ß, Cyrillic, CJK...) beyond a trace mean another language.
 */
export function looksEnglish(text: string): boolean {
	const letters = [...text].filter((c) => /\p{L}/u.test(c));
	if (letters.length === 0) return true;
	const other = letters.filter((c) => !/[a-zA-Z]/.test(c)).length;
	return other / letters.length < 0.02;
}

class Reader {
	/** The answer being prepared or played, by message id. */
	id = $state<number | null>(null);
	state = $state<State>('idle');
	error = $state<string | null>(null);

	#audio: HTMLAudioElement | null = null;
	#abort: AbortController | null = null;
	#urls = new Map<number, string>();

	/** Plays (or prepares and plays) the answer `id`; pressing it again while it plays stops it. */
	async toggle(id: number, model: string, text: string) {
		if (this.id === id && this.state !== 'idle') {
			this.stop();
			return;
		}
		this.stop();
		this.id = id;
		this.error = null;
		let url = this.#urls.get(id);
		if (!url) {
			this.state = 'preparing';
			const abort = (this.#abort = new AbortController());
			try {
				const reading = await speak(model, text, abort.signal);
				if (abort.signal.aborted) return;
				url = URL.createObjectURL(reading.audio);
				this.#remember(id, url);
				track('answer_read_aloud', { model, audio_seconds: Math.round(reading.audioSeconds), seconds: Math.round(reading.seconds * 10) / 10 });
			} catch (e) {
				if (abort.signal.aborted) return;
				this.error = e instanceof Error ? e.message : String(e);
				this.state = 'idle';
				return;
			} finally {
				if (this.#abort === abort) this.#abort = null;
			}
		}
		const audio = (this.#audio = new Audio(url));
		audio.onended = () => {
			if (this.#audio === audio) this.#reset();
		};
		this.state = 'playing';
		try {
			await audio.play();
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			this.#reset();
		}
	}

	stop() {
		this.#abort?.abort();
		this.#abort = null;
		this.#audio?.pause();
		this.#reset();
	}

	#reset() {
		this.#audio = null;
		this.id = null;
		this.state = 'idle';
	}

	#remember(id: number, url: string) {
		this.#urls.set(id, url);
		while (this.#urls.size > KEEP) {
			const [oldest, old] = this.#urls.entries().next().value as [number, string];
			URL.revokeObjectURL(old);
			this.#urls.delete(oldest);
		}
	}
}

export const reader = new Reader();
