<script lang="ts">
	import { chat, type ToolNote } from '#lib/chat.svelte.ts';

	// One quiet line in the conversation for a skill the model read or a connector tool it
	// used. A connector tool that needs a yes stops here and waits: the person sees the
	// connector, the tool and the arguments, and answers with Allow or Deny.

	let { note }: { note: ToolNote } = $props();

	const line = $derived.by(() => {
		const asked = note.state === 'denied' || note.state === 'stopped';
		switch (note.kind) {
			case 'skill':
				return `Read the skill ${note.what}`;
			case 'file':
				return `Read ${note.what}`;
			case 'find':
				return `Looked for a tool: “${note.what}”`;
			default:
				return `${note.state === 'asking' ? 'Wants to use' : asked ? 'Asked to use' : note.state === 'failed' ? 'Tried to use' : 'Used'} ${note.what} from the connector ${note.connector ?? ''}`;
		}
	});

	const tails: Record<ToolNote['state'], string> = { asking: 'waiting for you', running: 'working', done: '', failed: 'did not work', denied: 'not allowed', stopped: 'stopped' };
</script>

{#if note.state === 'asking'}
	<div class="ask" role="group" aria-label="Waiting for your answer">
		<h2>Use {note.what} from the connector {note.connector}?</h2>
		<p class="cap">With these arguments</p>
		<pre class="selectable">{note.arguments}</pre>
		<p class="why">This tool belongs to the connector, not to Rigbrew. What is sent to it leaves this chat. Allow it only if you know what it does.</p>
		<div class="answers">
			<button class="btn primary" onclick={() => chat.answerTool(note.callId, true)}>Allow</button>
			<button class="btn" onclick={() => chat.answerTool(note.callId, false)}>Deny</button>
			<span class="grow"></span>
			<button class="btn quiet" onclick={() => chat.answerTool(note.callId, true, true)} title="Uses this tool without asking again, in every chat. You can change it under Connectors.">Always allow this tool</button>
		</div>
	</div>
{:else}
	<details class="step {note.state}">
		<summary>
			<span class="mark" aria-hidden="true"></span>
			<span class="did">{line}</span>
			{#if tails[note.state]}<span class="tail">{tails[note.state]}</span>{/if}
		</summary>
		<div class="inside">
			{#if note.arguments}
				<p class="cap">Sent</p>
				<pre class="selectable">{note.arguments}</pre>
			{/if}
			{#if note.result}
				<p class="cap">{note.kind === 'skill' || note.kind === 'file' ? 'What the model read' : 'What came back'}</p>
				<pre class="selectable">{note.result}</pre>
			{/if}
		</div>
	</details>
{/if}

<style>
	.step {
		font-family: var(--rb-text);
		font-size: 13px;
		color: var(--muted);
	}

	summary {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 26px;
		list-style: none;
		cursor: pointer;
	}

	summary::-webkit-details-marker {
		display: none;
	}

	summary:hover .did {
		color: var(--ink);
	}

	.mark {
		flex: none;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--line);
	}

	.done .mark {
		background: var(--fit);
	}

	.failed .mark {
		background: var(--fail);
	}

	.running .mark {
		background: var(--accent);
		animation: blink 1.2s steps(2, jump-none) infinite;
	}

	.did {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.tail {
		flex: none;
		font-size: 12px;
		color: var(--faint);
	}

	.failed .tail {
		color: var(--fail);
	}

	.inside {
		display: grid;
		gap: 6px;
		margin: 4px 0 8px 2px;
		padding-left: 12px;
		border-left: 1px solid var(--line-soft);
	}

	.cap {
		font-size: 12px;
		color: var(--faint);
	}

	pre {
		margin: 0;
		padding: 8px 10px;
		border: 1px solid var(--line-soft);
		border-radius: 8px;
		background: var(--well);
		font: 12.5px/1.5 var(--rb-data);
		color: var(--ink);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		max-height: 220px;
		overflow: auto;
	}

	.ask {
		display: grid;
		gap: 8px;
		padding: 14px 16px;
		border: 1px solid var(--line);
		border-radius: 12px;
		font-family: var(--rb-text);
	}

	h2 {
		font: 600 14px/1.4 var(--rb-text);
		letter-spacing: 0;
		overflow-wrap: anywhere;
	}

	.why {
		font-size: 12.5px;
		color: var(--muted);
	}

	.answers {
		display: flex;
		align-items: center;
		gap: 6px;
		flex-wrap: wrap;
	}

	.grow {
		flex: 1;
	}
</style>
