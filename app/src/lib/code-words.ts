// The words Code shows while it works: one at a time, above the box to type in. They are
// Rigbrew's own: a brewing rig and a kitchen lab, with a programmer's dry sense of humour.
// Each pool belongs to something that is happening; `thinking` is also what is shown when
// nothing more is known. Keep them short (one to three words, four at most), kind, and free of
// names of people, products and companies.

/** What the model is doing right now, as far as the screen can tell. */
export type Activity = 'thinking' | 'searching' | 'reading' | 'writing' | 'running' | 'web';

export const words: Record<Activity, readonly string[]> = {
	thinking: [
		'Steeping…',
		'Letting it bloom…',
		'Warming the kettle…',
		'Reading the tea leaves…',
		'Consulting the dregs…',
		'Skimming the foam…',
		'Tasting the wort…',
		'Waiting on the yeast…',
		'Weighing the hops…',
		'Decanting thoughts…',
		'Sniffing the cork…',
		'Watching the pot…',
		'Fermenting an idea…',
		'Bottling a hunch…',
		'Checking the recipe…',
		'Tuning the rig…',
		'Oiling the gears…',
		'Priming the pump…',
		'Polishing the beakers…',
		'Adjusting the goggles…',
		'Calibrating the hunch…',
		'Stirring clockwise…',
		'Stirring the other way…',
		'Letting it rest…',
		'Proofing the dough…',
		'Sipping slowly…',
		'Counting to ten…',
		'Counting from zero…',
		'Carrying the one…',
		'Staring at the ceiling…',
		'Asking the rubber duck…',
		'Pacing the lab…',
		'Sharpening pencils…',
		'Chewing the pencil…',
		'Drawing on napkins…',
		'Arguing with myself…',
		'Holding that thought…',
		'Squinting thoughtfully…',
		'Naming things…',
		'Off by one…',
		'Balancing brackets…',
		'Corralling semicolons…',
		'Untangling spaghetti…',
		'Negotiating with entropy…',
		'Reheating an old idea…',
		'Summoning patience…',
		'Thinking in tabs…',
		'Thinking in spaces…',
		'Measuring twice…',
		'Topping up the mash…'
	],
	searching: [
		'Rummaging…',
		'Raiding the pantry…',
		'Checking under the rug…',
		'Following the breadcrumbs…',
		'Sifting the grounds…',
		'Panning for gold…',
		'Lifting every lid…',
		'Opening all the drawers…',
		'Shaking the haystack…',
		'Looking for the needle…',
		'Interrogating folders…',
		'Hunting a semicolon…',
		'Looking behind the fridge…',
		'Dusting for fingerprints…',
		'Asking around…',
		'Rounding up suspects…',
		'Turning over stones…',
		'Sniffing out a clue…',
		'Straining the pulp…',
		'Checking the back shelf…',
		'Reading the labels…',
		'Emptying the junk drawer…'
	],
	reading: [
		'Reading the fine print…',
		'Squinting at code…',
		'Decoding the handwriting…',
		'Reading between lines…',
		'Admiring old comments…',
		'Trusting the comments…',
		'Doubting the comments…',
		'Meeting the legacy…',
		'Tracing the wires…',
		'Following the plumbing…',
		'Studying the recipe…',
		'Counting the indents…',
		'Scrolling, scrolling…',
		'Nodding along…',
		'Taking notes…',
		'Sampling the batch…',
		'Inspecting the sediment…',
		'Unfolding the map…',
		'Actually reading it…',
		'Skimming the label…',
		'Minding the braces…',
		'Meeting the neighbours…'
	],
	writing: [
		'Pouring carefully…',
		'Bottling it…',
		'Corking the bottle…',
		'Labelling jars…',
		'Plating up…',
		'Adding a pinch…',
		'Seasoning to taste…',
		'Two-finger typing…',
		'Moving semicolons…',
		'Aligning the braces…',
		'Indenting with feeling…',
		'Renaming it again…',
		'Deleting with confidence…',
		'Welding the seams…',
		'Soldering loose ends…',
		'Leaving a comment…',
		'Filling the kegs…',
		'Frosting the cake…',
		'Kneading the code…',
		'Garnishing…',
		'Folding in gently…',
		'Laying bricks…',
		'Topping with tests…',
		'Tightening the lid…'
	],
	running: [
		'Bribing the compiler…',
		'Blaming the cache…',
		'Lighting the burner…',
		'Flipping the switch…',
		'Pulling the lever…',
		'Cranking the handle…',
		'Crossing fingers…',
		'Holding my breath…',
		'Appeasing the linter…',
		'Feeding the tests…',
		'Waking the build…',
		'Poking it gently…',
		'Kicking the tyres…',
		'Watching the dots…',
		'Rattling the pipes…',
		'Venting steam…',
		'Checking the gauges…',
		'Hoping for zero…',
		'Firing the kiln…',
		'Pressure testing…',
		'Shaking the flask…',
		'Turning up the heat…',
		'Listening for the ping…',
		'Bracing for red…'
	],
	web: [
		'Dialling out…',
		'Asking the internet…',
		'Waiting for packets…',
		'Untangling the web…',
		'Fetching fresh water…',
		'Borrowing some sugar…',
		'Phoning a friend…',
		'Skipping the ads…',
		'Closing cookie banners…',
		'Dodging pop-ups…',
		'Checking the almanac…',
		'Importing rare spices…',
		'Shopping for ingredients…',
		'Queuing politely…',
		'Pinging the void…',
		'Listening for echoes…',
		'Sending a pigeon…',
		'Checking the mailbox…',
		'Tuning the antenna…',
		'Knocking on servers…',
		'Reading the room…',
		'Asking a stranger…'
	]
};

/** How long one word stays, and how soon it may change when something else starts happening. */
export const WORD_MS = 4000;
export const WORD_MIN_MS = 1500;
/** A word is not shown again until this many others were. */
export const NO_REPEAT = 8;

/**
 * The next word: mostly from the pool of what is happening, now and then from the general one,
 * and never one of the last few. `random` gives numbers from 0 up to 1 (not 1).
 */
export function nextWord(activity: Activity, recent: readonly string[], random: () => number = Math.random): string {
	const own = words[activity] ?? words.thinking;
	const pool = activity !== 'thinking' && random() < 0.2 ? words.thinking : own;
	const last = recent.slice(-NO_REPEAT);
	const fresh = pool.filter((w) => !last.includes(w));
	const from = fresh.length ? fresh : pool;
	return from[Math.min(from.length - 1, Math.floor(random() * from.length))];
}

/** The time a request has taken so far: "8s", "2m 05s", "1h 03m". */
export function elapsed(ms: number): string {
	const s = Math.max(0, Math.floor(ms / 1000));
	const two = (n: number) => String(n).padStart(2, '0');
	if (s < 60) return `${s}s`;
	if (s < 3600) return `${Math.floor(s / 60)}m ${two(s % 60)}s`;
	return `${Math.floor(s / 3600)}h ${two(Math.floor((s % 3600) / 60))}m`;
}
