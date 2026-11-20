// The help articles. Plain English, a few sentences each, written for someone who has never
// used an AI tool. Edit the words here; Help.svelte only lays them out and searches them.
// "?" marks in the interface open an article by its id (components/Hint.svelte).

import { machine, mod } from './words';

export type HelpAction = 'tour' | 'privacy' | 'models' | 'device' | 'settings' | 'updates';

export interface Term {
	term: string;
	text: string;
}

export interface Article {
	id: string;
	title: string;
	/** Paragraphs. */
	body: string[];
	/** A list of terms (the glossary) or of keys (shortcuts), after the paragraphs. */
	terms?: Term[];
	/** Buttons under the article that do what it talks about. */
	actions?: { label: string; does: HelpAction }[];
}

const Mac = machine;

export const articles: Article[] = [
	{
		id: 'about',
		title: 'What Rigbrew is',
		body: [
			`Rigbrew runs AI models on this ${Mac}. You type a question or describe a picture, and a model on your own machine answers. Getting the answer does not need the internet.`,
			`It reads what this ${Mac} can handle, picks the models that suit it and installs them for you. One chat window uses all of them.`,
			'Rigbrew is free and open source. In return it collects how the app is used and, with your consent, what you type, to make the models better. “Where my data goes” says exactly what.'
		],
		actions: [{ label: 'Take the tour', does: 'tour' }]
	},
	{
		id: 'models',
		title: 'What a model is, and what each kind does',
		body: [
			`A model is a large file that has learned from many examples how to do one kind of thing, such as answering in words or drawing a picture. Rigbrew loads it into your ${Mac}’s memory and runs it there.`,
			'Chat (Steep) answers questions, writes and explains. Pictures (Crema) makes an image from a description. Video (Pour), Voice (Whistle) and Listening (Sift) can be downloaded today; running them in Rigbrew comes later.',
			'Each kind installs separately, and the installed ones work together in the same chat: ask Chat for a drawing and it hands the job to Pictures.',
			'The name after the kind is the model’s own name and its size: Steep 4B has about 4 billion learned numbers.'
		],
		actions: [{ label: 'Open Models', does: 'models' }]
	},
	{
		id: 'one-model',
		title: 'Why only one model is in memory at a time',
		body: [
			`A model has to sit in memory to work, and a large one takes several gigabytes. To leave room for your other apps, Rigbrew keeps one model in memory at a time.`,
			'When you go from Chat to Picture, Rigbrew moves the picture model in and the chat model out. That takes a few seconds, and the line at the bottom of the window shows it happening.',
			'That line always says which model is loaded and how much room is left. Click it to unload the model and free the memory now; the next message loads it again.'
		]
	},
	{
		id: 'fits',
		title: `What “fits this ${Mac}” means`,
		body: [
			'Rigbrew measures every model it offers: how much memory it needs while it works, and how close its answers are to the original.',
			`On first start it reads this ${Mac}: how much memory there is, and how much can go to models while enough stays free for the system and your other apps. The Your ${Mac} page shows both numbers.`,
			'A model fits when it needs less than that share. Green always means “fits”. Rigbrew recommends only what fits, and offers no Install button for what does not.'
		],
		actions: [{ label: `See your ${Mac}`, does: 'device' }]
	},
	{
		id: 'versions',
		title: 'Versions: smaller or better',
		body: [
			'Most models come in several versions. A smaller version needs less memory and disk space and is often faster; a larger one gives answers closer to the original model.',
			`Rigbrew measures each version against the original and recommends the smallest one that keeps the quality high and fits this ${Mac}. That is the version marked “Recommended”, and the one Guided installs.`,
			'Standard shows the other versions (Smallest, Balanced, Best quality) under each model, so you can install a different one. When two versions are installed, Rigbrew uses the better one.',
			'Pro shows the raw names (q4, q5, int8) and the measurements behind them.'
		],
		actions: [{ label: 'Open Models', does: 'models' }]
	},
	{
		id: 'pictures',
		title: 'Making pictures',
		body: [
			'Switch the box at the bottom of the chat to Picture and describe what you want to see: the subject, the setting, the style, the light. Rigbrew makes one picture from it.',
			'You can also ask in Chat, for example “draw a lighthouse at night”. The chat model then asks the picture model to make it. Picture mode is quicker, because only one model has to be loaded.',
			'A picture takes tens of seconds, longer the first time while the picture model is loaded. Each picture is kept with its chat. Save puts a copy where you choose; Try again makes a new one from the same words.',
			'Pictures need Pictures (Crema) to be installed. If it is not, Rigbrew offers the install right in the chat.'
		]
	},
	{
		id: 'first-answer',
		title: 'Why the first answer is slower',
		body: [
			'Before a model can answer, it is loaded from disk into memory. That happens on the first message after Rigbrew starts, and when you switch between Chat and Picture.',
			'Loading takes from a second to half a minute, depending on the model’s size. After that, answers start almost at once.',
			'While you wait, the chat says what is happening (“Loading…” or “Moving … into memory”) with a running clock.'
		]
	},
	{
		id: 'data',
		title: 'Where my data goes',
		body: [
			`The models run on this ${Mac} and need no internet to answer. What Rigbrew sends has two layers.`,
			'Only with your consent, and only while it stands: the text of your prompts and of the answers and your picture descriptions, linked to your Rigbrew account (anonymous until you sign in). Pictures, sound and video are never uploaded.',
			'Always: basic usage data, such as screens and main buttons used, models installed and loaded, how many chats and pictures, speeds, errors, the hardware class and the app version, with a random install number. It never holds what you type or what a model answers, is not linked to your account, and keeps the country your connection comes from but not the IP address. Developers can switch it off with the environment variable DO_NOT_TRACK=1.',
			`Privacy and data lists every event waiting on this ${Mac} and marks the ones that carry content. Withdrawing consent stops the content and removes it from what is not yet sent; basic usage data continues.`
		],
		actions: [{ label: 'Open Privacy and data', does: 'privacy' }]
	},
	{
		id: 'disk',
		title: 'Disk space and removing models',
		body: [
			`Models are large files: a chat model takes about 3 GB, a picture model about 8 GB. They are kept in Rigbrew’s own folder on this ${Mac}; Settings shows where.`,
			'Every Install button shows the download size first. A download can be cancelled; what is already downloaded stays, so installing again continues from there.',
			'To free space, open Models and choose Remove next to a model. Rigbrew says how much that frees before it removes anything.'
		],
		actions: [{ label: 'Open Models', does: 'models' }]
	},
	{
		id: 'updates',
		title: 'Updates',
		body: [
			'Rigbrew checks for a new version when it starts and every few hours. When there is one, a small notice appears in the corner, and Update opens the download page.',
			'Rarely, an old version can no longer be used. Rigbrew then asks you to update before going on. Your models, chats and settings stay where they are.',
			'Settings shows the version you have and can check right now.'
		],
		actions: [{ label: 'Check for updates', does: 'updates' }]
	},
	{
		id: 'chats',
		title: 'Your chats',
		body: [
			`Chats are saved on this ${Mac} after every answer. The list beside the chat and the search (${mod}K) show them.`,
			'A chat is named after its first message. Click the name above a chat to change it.',
			`Delete removes a chat and its pictures from this ${Mac}. For a few seconds after, Undo brings it back.`
		]
	},
	{
		id: 'levels',
		title: 'Guided, Standard and Pro',
		body: [
			'Rigbrew can show more or less. Guided uses plain words, one button per model, tips and a checklist. Standard adds model versions, speeds, the memory ruler and today’s figures. Pro shows everything: every build with its measurements, raw ids, shortcuts and the queue of usage data.',
			`Change it any time in Settings, or by searching for “Guided”, “Standard” or “Pro” with ${mod}K. Switching installs and removes nothing.`,
			'Rigbrew may suggest the next level once you use it more. It never switches by itself.'
		],
		actions: [{ label: 'Open Settings', does: 'settings' }]
	},
	{
		id: 'keys',
		title: 'Keyboard shortcuts',
		body: ['Everything also works with the pointer. These save time once you know them.'],
		terms: [
			{ term: 'Enter', text: 'Send. Shift+Enter starts a new line.' },
			{ term: 'Esc', text: 'Stop the answer or the picture; close a sheet.' },
			{ term: `${mod}K`, text: 'Search chats, models and actions.' },
			{ term: `${mod}N`, text: 'New chat.' },
			{ term: `${mod}1, 2, 3`, text: `Chat, Models, Your ${Mac}.` },
			{ term: `${mod}⇧I`, text: 'Switch between Chat and Picture.' },
			{ term: `${mod}⇧U`, text: 'Unload the model from memory.' },
			{ term: `${mod}\\`, text: 'Show or hide the list beside the chat.' },
			{ term: `${mod},`, text: 'Settings.' },
			{ term: `${mod}/ or F1`, text: 'Help.' },
			{ term: '/', text: 'At the start of a message: /image, /chat, /model, /new, /unload, /models.' }
		]
	},
	{
		id: 'glossary',
		title: 'Words used in Rigbrew',
		body: ['The few technical words the app uses, in one place.'],
		terms: [
			{ term: 'Model', text: 'A large file that has learned to do one kind of thing, such as answering or drawing. Rigbrew runs it on this machine.' },
			{ term: 'Version (build)', text: 'One way of packing a model: smaller versions need less memory, larger ones stay closer to the original. Pro calls them builds.' },
			{ term: 'Memory', text: `The working memory (RAM) of this ${Mac}. A model has to be in memory to work, and only one is at a time.` },
			{ term: 'Fits', text: `Needs less memory than this ${Mac} can give models while staying responsive. Green always means this.` },
			{ term: 'Tokens', text: 'The pieces a model reads and writes: a token is a short word or part of a longer one. About 100 tokens make 75 English words.' },
			{ term: 'Tokens per second', text: 'How fast a model writes. 20 or more reads as fast; 50 is faster than you can read.' },
			{ term: 'Quantization', text: 'Storing a model’s numbers with fewer digits so it is smaller. The q4, q5, q8 and int8 in Pro are levels of it: the number is roughly the bits per value.' },
			{ term: 'Loaded', text: 'In memory and ready to answer at once. The line at the bottom of the window shows what is loaded.' },
			{ term: 'Usage data', text: 'What Rigbrew records about how the app is used. Basic usage data (no text you wrote) is always sent; prompts and answers only with your consent. Privacy and data lists it.' }
		]
	}
];

export const articleById = (id: string) => articles.find((a) => a.id === id);

/** Every word of an article, for the search. */
export const searchText = (a: Article) => [a.title, ...a.body, ...(a.terms ?? []).flatMap((t) => [t.term, t.text])].join(' ').toLowerCase();
