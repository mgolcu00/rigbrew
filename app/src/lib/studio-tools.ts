// Every tool of the Studio by its id: its name, what it does in one short line, and its key. The
// help tags on the controls and the list of keys and tools are both made from this list, so the
// two always say the same thing.

import { mod } from './words.ts';

export type ToolGroup = 'Transport' | 'Time' | 'Make' | 'Mix' | 'View' | 'Edit' | 'Tracks' | 'Library' | 'Inspector' | 'Editor' | 'Cast' | 'Assistant' | 'Pointer';

export interface Tool {
	id: string;
	name: string;
	what: string;
	key?: string;
	group: ToolGroup;
}

/** The groups in the order the list of keys and tools shows them. */
export const GROUPS: ToolGroup[] = ['Transport', 'Time', 'Make', 'Mix', 'View', 'Edit', 'Tracks', 'Library', 'Inspector', 'Editor', 'Cast', 'Assistant', 'Pointer'];

const t = (group: ToolGroup, id: string, name: string, what: string, key?: string): Tool => ({ id, name, what, key, group });

export const TOOLS: Tool[] = [
	t('Transport', 'start', 'Go to start', 'Moves the playhead to the start of the film.', 'Return'),
	t('Transport', 'play', 'Play or pause', 'Plays the timeline from the playhead.', 'Space'),
	t('Transport', 'stop', 'Stop', 'Stops; pressed again, goes back to the start.'),
	t('Transport', 'cycle', 'Cycle', 'Plays the cycle range over and over.', 'C'),
	t('Transport', 'shot-step', 'Shot before or after', 'Selects it and moves the playhead to its start.', '← →'),
	t('Transport', 'frame-step', 'Frame back or on', 'Moves the playhead by one frame.', ', .'),
	t('Transport', 'second-step', 'Second back or on', 'Moves the playhead by one second.', 'Shift , .'),
	t('Transport', 'ends', 'Start or end', 'Moves the playhead to the start or the end.', 'Home End'),

	t('Time', 'display', 'Display', 'Time at the playhead; click to show the shot instead.'),
	t('Time', 'length', 'Length and made', 'How long the film is, and how many shots are made.'),
	t('Time', 'ruler', 'Ruler', 'Click or drag to move the playhead; its top edge sets the cycle range.'),
	t('Time', 'zoom', 'Zoom', 'Shows more or less of the film across the window.', `${mod}← →`),
	t('Time', 'fit', 'Fit the film', 'Zooms so the whole film fits the window.', 'Z'),
	t('Time', 'snap', 'Snap', 'Edges land on frames, other edges and the playhead. Alt places freely.'),

	t('Make', 'make', 'Make shots', 'Makes the shots that are not made yet.', `${mod}Return`),
	t('Make', 'stop-making', 'Stop making', 'Stops the shot being made; made shots stay.'),
	t('Make', 'play-film', 'Play film', 'Plays the whole film, large.', 'P'),
	t('Make', 'export', 'Export', 'Joins the made shots and the sound into one MP4 file.', `${mod}B`),

	t('Mix', 'master', 'Master volume', 'How loud the whole film is.'),
	t('Mix', 'mixer', 'Mixer', 'Volume, pan, mute and solo of every track.', 'X'),
	t('Mix', 'mute', 'Mute', 'Silences this track.', 'M'),
	t('Mix', 'solo', 'Solo', 'Only soloed tracks are heard.', 'S'),
	t('Mix', 'volume', 'Volume', 'Drag to change; double-click for 0 dB.'),
	t('Mix', 'pan', 'Pan', 'Left or right; double-click for the middle.'),
	t('Mix', 'meter', 'Meter', 'How loud the track is right now.'),

	t('View', 'library', 'Library', 'Films, takes and sound files.', 'Y'),
	t('View', 'inspector', 'Inspector', 'Settings of what is selected.', 'I'),
	t('View', 'assistant', 'Assistant', 'Ask for a change to the film in your own words.', 'A'),
	t('View', 'editor', 'Editor', 'The selected shot or sound up close, for trims and fades.', 'E'),
	t('View', 'cast', 'Cast', 'The characters, and how each one looks.', 'K'),
	t('View', 'viewer', 'Large viewer', 'Shows the picture large over the window.', 'V'),
	t('View', 'heights', 'Track height', 'Makes every track taller or lower.', `${mod}↑ ↓`),
	t('View', 'keys', 'Keys and tools', 'Every tool, what it does and its key.', '?'),

	t('Edit', 'undo', 'Undo', 'Takes back the last change.', `${mod}Z`),
	t('Edit', 'redo', 'Redo', 'Does the change again.', `Shift ${mod}Z`),
	t('Edit', 'delete', 'Delete', 'Deletes what is selected.', 'Delete'),
	t('Edit', 'select-all', 'Select all', 'Everything on the selected track.', `${mod}A`),
	t('Edit', 'duplicate', 'Duplicate', 'A copy right after what is selected.', `${mod}D`),
	t('Edit', 'copy-paste', 'Copy and paste', 'Sound regions, pasted at the playhead.', `${mod}C ${mod}V`),
	t('Edit', 'split', 'Split', 'Cuts at the playhead.', `${mod}T`),

	t('Tracks', 'add-track', 'Add track', 'A new sound track under the others.'),
	t('Tracks', 'add-shot', 'Add shot', 'A new shot at the end of the film.'),
	t('Tracks', 'track-color', 'Track colour', 'Click for the next colour.'),
	t('Tracks', 'track-name', 'Track name', 'Double-click to rename.'),
	t('Tracks', 'track-move', 'Move a track', 'Drag its header up or down.'),

	t('Library', 'new-film', 'New film', 'Starts a film from a few words, in detail, or from a video.'),
	t('Library', 'empty-film', 'Empty film', 'A film without shots, or a film folder to open.'),
	t('Library', 'bring-video', 'Bring in a video', 'Cuts a video file into shots at the end of the film.'),
	t('Library', 'add-sound', 'Add a sound file', 'Puts a sound file in the library, to drag onto a track.'),
	t('Library', 'add-folder', 'Add a folder', 'Every sound file in a folder.'),
	t('Library', 'listen', 'Listen', 'Plays the sound file by itself.'),
	t('Library', 'take', 'Take', 'Drag onto its shot, or double-click, to use it.'),
	t('Library', 'remove-film', 'Remove from list', 'The film’s folder and files stay.'),

	t('Inspector', 'description', 'Description', 'What the shot shows: the video model makes it from this.'),
	t('Inspector', 'rewrite', 'Write again', 'The chat model writes the description again.'),
	t('Inspector', 'make-shot', 'Make this shot', 'Makes the selected shots only.'),
	t('Inspector', 'continues', 'Continues', 'Starts from the last picture of the shot before.'),
	t('Inspector', 'join', 'Join', 'Cut, dissolve or fade through black into this shot.'),
	t('Inspector', 'write-shots', 'Write shots', 'The chat model writes shots from an idea.'),
	t('Inspector', 'narrate', 'Narrate', 'The voice reads a text onto a track at the playhead.'),
	t('Inspector', 'dub', 'Dub', 'The voice reads every line, in another language if asked.'),

	t('Editor', 'trim', 'Trim', 'Drag an edge of the clip to cut its start or end.'),
	t('Editor', 'fade', 'Fade', 'Drag a top corner of a region to fade it in or out.'),

	t('Cast', 'add-character', 'Add a character', 'A name and a look every shot naming them is made with.'),
	t('Cast', 'portrait', 'Portrait', 'A picture made from the look, to start shots from.'),
	t('Cast', 'write-look', 'Write the look', 'The chat model writes the look from a few words.'),

	t('Assistant', 'ask', 'Ask', 'Sends your request; the assistant edits the film step by step.', 'Return'),
	t('Assistant', 'stop-assistant', 'Stop', 'Stops the assistant; what it did stays.', 'Esc Esc'),
	t('Assistant', 'undo-request', 'Undo the request', 'Takes back everything the request changed, as one step.'),

	t('Pointer', 'pinch', 'Zoom around the pointer', 'Pinch, or Ctrl and the wheel.'),
	t('Pointer', 'scroll', 'Scroll along the film', 'Shift and the wheel.'),
	t('Pointer', 'free', 'Place freely', 'Hold Alt while dragging: nothing snaps.'),
	t('Pointer', 'alt-copy', 'Copy a region', 'Alt-drag it.'),
	t('Pointer', 'open-editor', 'Open in the Editor', 'Double-click a shot or a region.')
];

const byId = new Map(TOOLS.map((tool) => [tool.id, tool]));

export function tool(id: string): Tool | undefined {
	return byId.get(id);
}

/** Tools whose name, line, key or group holds every word typed, in the order of the list. */
export function findTools(query: string): Tool[] {
	const words = query.toLowerCase().split(/\s+/).filter(Boolean);
	if (words.length === 0) return TOOLS;
	return TOOLS.filter((x) => {
		const text = `${x.name} ${x.what} ${x.key ?? ''} ${x.group}`.toLowerCase();
		return words.every((w) => text.includes(w));
	});
}

/** A help tag: a tool by id, or one with its own words, or a tool with some of its words changed. */
export type TagSpec = string | { id?: string; name?: string; what?: string; key?: string };

export interface Tag {
	name: string;
	what: string;
	key: string;
}

export function tagOf(spec: TagSpec): Tag | null {
	const given = typeof spec === 'string' ? { id: spec } : spec;
	const base = given.id ? tool(given.id) : undefined;
	const name = given.name ?? base?.name ?? '';
	if (!name) return null;
	return { name, what: given.what ?? base?.what ?? '', key: given.key ?? base?.key ?? '' };
}

/**
 * A help tag from a control's own title, for controls that have no tag of their own: "Name: what
 * it does (Key)", or a sentence. Its name is the control's label when the title is one sentence.
 */
export function tagFromTitle(title: string, label = ''): Tag | null {
	let text = title.trim();
	if (!text) return null;
	let key = '';
	const keyed = text.match(/\s*\(([^()]{1,24})\)\.?$/);
	if (keyed && !/\s{2,}|\b(the|a|to|of|and|is)\b/i.test(keyed[1])) {
		key = keyed[1];
		text = text.slice(0, keyed.index).trim();
	}
	const named = text.match(/^([^.:]{1,32}):\s+(.+)$/);
	if (named) return { name: named[1], what: named[2], key };
	const name = label.trim();
	if (name && name.toLowerCase() !== text.toLowerCase()) return { name, what: text, key };
	return { name: text, what: '', key };
}
